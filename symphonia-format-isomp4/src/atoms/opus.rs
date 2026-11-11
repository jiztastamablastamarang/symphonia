// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use symphonia_common::xiph::audio::opus::{
    CHANNEL_MAPPING_FAMILY_RTP, ChannelMappingTable, OPUS_HEAD_VERSION, OpusHead, channel_positions,
};
use symphonia_core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia_core::errors::Error;
use symphonia_core::io::{BufReader, ReadBytes};

use crate::atoms::stsd::AudioSampleEntry;
use crate::atoms::{
    Atom, AtomHeader, AtomIterator, ReadAtom, Result, decode_error, unsupported_error,
};

/// The `Version` field of the Opus Specific Box (Encapsulation of Opus in ISO Base Media File
/// Format, Section 4.3.2).
const DOPS_VERSION: u8 = 0;

/// Size of the fixed fields of the box: version, output channel count, pre-skip, input sample
/// rate, output gain and channel mapping family.
const DOPS_FIXED_SIZE: usize = 11;

/// Size of the largest channel mapping table: stream count, coupled count and 255 entries.
const DOPS_MAX_TABLE_SIZE: usize = 2 + 255;

/// Opus atom (`dOps`).
#[derive(Debug)]
pub struct OpusAtom {
    /// The identification header the box carries.
    opus_head: OpusHead,
}

impl Atom for OpusAtom {
    fn read<R: ReadAtom>(reader: &mut AtomIterator<R>, header: &AtomHeader) -> Result<Self> {
        let data_len = header
            .data_size()
            .ok_or(Error::DecodeError("isomp4 (opus): expected atom size to be known"))?
            as usize;

        if data_len < DOPS_FIXED_SIZE {
            return decode_error("isomp4 (opus): opus identification header too short");
        }

        if data_len > DOPS_FIXED_SIZE + DOPS_MAX_TABLE_SIZE {
            return decode_error("isomp4 (opus): opus identification header too large");
        }

        let mut data = vec![0; data_len].into_boxed_slice();
        reader.read_buf_exact(&mut data)?;

        Ok(OpusAtom { opus_head: read_opus_specific_box(&data)? })
    }
}

/// Reads the Opus Specific Box: the fields of the `OpusHead` identification header, stored in
/// big-endian order and without the magic signature (Encapsulation of Opus in ISO Base Media
/// File Format, Section 4.3.2), into a header in the RFC 7845 layout.
fn read_opus_specific_box(data: &[u8]) -> Result<OpusHead> {
    let mut reader = BufReader::new(data);

    let version = reader.read_byte()?;
    if version != DOPS_VERSION {
        return unsupported_error("isomp4 (opus): unsupported opus version");
    }

    let channel_count = reader.read_byte()?;
    if channel_count == 0 {
        return decode_error("isomp4 (opus): invalid channel count");
    }

    let pre_skip = reader.read_be_u16()?;
    let original_sample_rate = reader.read_be_u32()?;
    let gain = reader.read_be_i16()?;
    let channel_mapping_family = reader.read_byte()?;
    let channels = channel_positions(channel_mapping_family, channel_count)?;

    // The channel mapping table is byte-oriented and shares the Ogg layout.
    let channel_mapping_table = match channel_mapping_family {
        CHANNEL_MAPPING_FAMILY_RTP => None,
        _ => Some(ChannelMappingTable::read(&mut reader, channel_count)?),
    };

    Ok(OpusHead {
        version: OPUS_HEAD_VERSION,
        channel_count,
        channels,
        original_sample_rate,
        gain,
        pre_skip,
        channel_mapping_family,
        channel_mapping_table,
    })
}

impl OpusAtom {
    pub fn fill_audio_sample_entry(self, entry: &mut AudioSampleEntry) {
        entry.codec_id = CODEC_ID_OPUS;
        entry.sample_rate = 48_000.0;
        // The extra data is the canonical OpusHead packet, the same layout Ogg and Matroska hand
        // to the decoder.
        entry.extra_data = Some(self.opus_head.to_bytes());
        entry.channels = Some(self.opus_head.channels);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use symphonia_core::audio::{Channels, Position};
    use symphonia_core::io::MediaSourceStream;

    use super::*;

    /// Reads one `dOps` atom with the given big-endian payload, or `None` when it is rejected.
    fn read_dops(payload: &[u8]) -> Option<OpusAtom> {
        let size = (AtomHeader::HEADER_SIZE as usize + payload.len()) as u32;
        let atom = [&size.to_be_bytes(), &b"dOps"[..], payload].concat();

        let source = MediaSourceStream::new(Box::new(Cursor::new(atom)), Default::default());
        let mut it = AtomIterator::new(source, None);
        it.next_header().ok()??;
        it.read_atom::<OpusAtom>().ok()
    }

    /// The fixed fields of a stereo box: pre-skip 312, 48 kHz input, gain -256 (-1 dB), family 0.
    const STEREO_DOPS: [u8; 11] = [0, 2, 0x01, 0x38, 0x00, 0x00, 0xbb, 0x80, 0xff, 0x00, 0];

    #[test]
    fn big_endian_box_fields_become_a_little_endian_opus_head() {
        let atom = read_dops(&STEREO_DOPS).unwrap();
        assert_eq!(atom.opus_head.pre_skip, 312);
        assert_eq!(atom.opus_head.original_sample_rate, 48_000);
        assert_eq!(atom.opus_head.gain, -256);

        let mut entry = AudioSampleEntry::default();
        atom.fill_audio_sample_entry(&mut entry);
        assert_eq!(entry.codec_id, CODEC_ID_OPUS);
        assert_eq!(entry.sample_rate, 48_000.0);
        let left_right = Position::FRONT_LEFT | Position::FRONT_RIGHT;
        assert_eq!(entry.channels, Some(Channels::Positioned(left_right)));
        assert_eq!(
            entry.extra_data.as_deref(),
            Some(
                &[
                    b'O', b'p', b'u', b's', b'H', b'e', b'a', b'd', 1, 2, 0x38, 0x01, 0x80, 0xbb,
                    0x00, 0x00, 0x00, 0xff, 0
                ][..]
            )
        );
    }

    #[test]
    fn channel_mapping_table_is_carried_over() {
        let payload =
            [&[0, 3, 0x01, 0x38, 0x00, 0x00, 0xbb, 0x80, 0x00, 0x00, 1][..], &[2, 1, 0, 2, 1]]
                .concat();
        let atom = read_dops(&payload).unwrap();
        let table = atom.opus_head.channel_mapping_table.as_ref().unwrap();
        assert_eq!((table.stream_count, table.coupled_count), (2, 1));
        assert_eq!(&*table.channel_mapping, &[0, 2, 1]);
        assert_eq!(&atom.opus_head.to_bytes()[19..], &[2, 1, 0, 2, 1]);
    }

    #[test]
    fn unsupported_box_version_and_invalid_table_are_rejected() {
        let mut versioned = STEREO_DOPS;
        versioned[0] = 1;
        assert!(read_dops(&versioned).is_none());

        let bad_table =
            [&[0, 3, 0x01, 0x38, 0x00, 0x00, 0xbb, 0x80, 0x00, 0x00, 1][..], &[2, 3, 0, 2, 1]]
                .concat();
        assert!(read_dops(&bad_table).is_none());
    }
}
