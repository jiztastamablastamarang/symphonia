// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Opus identification header `OpusHead` (RFC 7845, Section 5.1).
//!
//! The header opens an Ogg Opus stream and is carried verbatim as the codec private data of a
//! Matroska Opus track. The ISO Base Media File Format carries the same fields in big-endian
//! order in its `dOps` box; [`OpusHead::to_bytes`] renders a header read from such a container
//! as the canonical little-endian packet so every consumer sees one layout.

use symphonia_core::{
    audio::{Channels, Position},
    errors::{Error, Result, decode_error, unsupported_error},
    io::ReadBytes,
};

const OPUS_MAGIC_SIGNATURE: &[u8] = b"OpusHead";

/// The `OpusHead` version of RFC 7845.
pub const OPUS_HEAD_VERSION: u8 = 1;

/// Channel mapping family 0: one stream of mono or left/right stereo and no mapping table
/// (RFC 7845, Section 5.1.1.1).
pub const CHANNEL_MAPPING_FAMILY_RTP: u8 = 0;

/// Channel mapping family 1: 1 to 8 channels in Vorbis order (RFC 7845, Section 5.1.1.2).
pub const CHANNEL_MAPPING_FAMILY_VORBIS: u8 = 1;

/// Channel mapping entry that marks an output channel as silent (RFC 7845, Section 5.1.1).
pub const SILENT_CHANNEL: u8 = 255;

/// The channel mapping table that follows the mapping family byte of every family other than 0
/// (RFC 7845, Section 5.1.1): `N` streams, `M` of them coupled stereo, decoded into `N + M`
/// channels and routed to the output channels by one entry per output channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelMappingTable {
    /// The `Stream Count` field `N`, the number of Opus streams in each packet.
    pub stream_count: u8,
    /// The `Coupled Stream Count` field `M`, the number of streams that decode to two channels.
    pub coupled_count: u8,
    /// The `Channel Mapping` field: for each output channel, the decoded channel it plays, or
    /// [`SILENT_CHANNEL`].
    pub channel_mapping: Box<[u8]>,
}

impl ChannelMappingTable {
    /// Reads the table of a header with `channel_count` output channels, rejecting the
    /// combinations RFC 7845, Section 5.1.1 forbids.
    pub fn read<B: ReadBytes>(reader: &mut B, channel_count: u8) -> Result<Self> {
        let stream_count = reader.read_byte()?;
        if stream_count == 0 {
            return decode_error("common (opus): invalid stream count");
        }

        let coupled_count = reader.read_byte()?;
        if coupled_count > stream_count {
            return decode_error("common (opus): coupled stream count exceeds stream count");
        }

        let decoded_channels = stream_count.checked_add(coupled_count).ok_or(
            Error::DecodeError("common (opus): stream count plus coupled count exceeds 255"),
        )?;

        let channel_mapping = reader.read_boxed_slice_exact(usize::from(channel_count))?;

        let out_of_range = channel_mapping
            .iter()
            .any(|&decoded| decoded != SILENT_CHANNEL && decoded >= decoded_channels);

        if out_of_range {
            return decode_error("common (opus): channel mapping names a missing decoded channel");
        }

        Ok(Self { stream_count, coupled_count, channel_mapping })
    }
}

/// The identification header `OpusHead` (RFC 7845, Section 5.1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpusHead {
    /// The `Version` field: [`OPUS_HEAD_VERSION`] for an Ogg Opus stream.
    pub version: u8,
    /// The `Output Channel Count` field.
    pub channel_count: u8,
    /// The output channel layout the mapping family assigns to `channel_count` channels.
    pub channels: Channels,
    /// The `Input Sample Rate` field in Hz, the rate of the original audio. The decoder output
    /// rate is 48 kHz regardless of this value.
    pub original_sample_rate: u32,
    /// The `Output Gain` field, a Q7.8 gain in dB to apply to the decoded output.
    pub gain: i16,
    /// The `Pre-skip` field, the number of samples at 48 kHz to discard from the decoder output
    /// at the start of playback.
    pub pre_skip: u16,
    /// The `Channel Mapping Family` field.
    pub channel_mapping_family: u8,
    /// The channel mapping table, present for every family other than
    /// [`CHANNEL_MAPPING_FAMILY_RTP`].
    pub channel_mapping_table: Option<ChannelMappingTable>,
}

impl OpusHead {
    /// Reads a header in its canonical little-endian layout. The max version is specified by
    /// the caller since it depends on the container format used.
    pub fn read<B: ReadBytes>(reader: &mut B, max_version: u8) -> Result<Self> {
        // The first 8 bytes are the magic signature ASCII bytes.
        let mut magic = [0; 8];
        reader.read_buf_exact(&mut magic)?;

        if magic != *OPUS_MAGIC_SIGNATURE {
            return unsupported_error("common (opus): invalid magic signature");
        }

        // The next byte is the encapsulation version.
        let version = reader.read_byte()?;
        if version > max_version {
            return decode_error("common (opus): invalid version");
        }

        // The next byte is the number of channels and must not be 0.
        let channel_count = reader.read_byte()?;

        if channel_count == 0 {
            return decode_error("common (opus): invalid channel count");
        }

        // The next 16-bit integer is the pre-skip padding (# of samples at 48kHz to subtract from
        // the OGG granule position to obtain the PCM sample position).
        let pre_skip = reader.read_u16()?;

        // The next 32-bit integer is the sample rate of the original audio.
        let original_sample_rate = reader.read_u32()?;

        // Next, the 16-bit gain value.
        let gain = reader.read_i16()?;

        // The next byte indicates the channel mapping family, followed by the channel mapping
        // table for every family other than 0.
        let channel_mapping_family = reader.read_byte()?;
        let channels = channel_positions(channel_mapping_family, channel_count)?;

        let channel_mapping_table = match channel_mapping_family {
            CHANNEL_MAPPING_FAMILY_RTP => None,
            _ => Some(ChannelMappingTable::read(reader, channel_count)?),
        };

        Ok(Self {
            version,
            channel_count,
            channels,
            original_sample_rate,
            gain,
            pre_skip,
            channel_mapping_family,
            channel_mapping_table,
        })
    }

    /// Renders the header as the canonical little-endian `OpusHead` packet.
    pub fn to_bytes(&self) -> Box<[u8]> {
        let mut bytes = Vec::with_capacity(OPUS_MAGIC_SIGNATURE.len() + 11 + 2 + 255);
        bytes.extend_from_slice(OPUS_MAGIC_SIGNATURE);
        bytes.push(self.version);
        bytes.push(self.channel_count);
        bytes.extend_from_slice(&self.pre_skip.to_le_bytes());
        bytes.extend_from_slice(&self.original_sample_rate.to_le_bytes());
        bytes.extend_from_slice(&self.gain.to_le_bytes());
        bytes.push(self.channel_mapping_family);

        if let Some(table) = &self.channel_mapping_table {
            bytes.push(table.stream_count);
            bytes.push(table.coupled_count);
            bytes.extend_from_slice(&table.channel_mapping);
        }

        bytes.into_boxed_slice()
    }
}

/// The output channel layout a channel mapping family assigns to `channel_count` channels
/// (RFC 7845, Section 5.1.1): mono or left/right stereo for family 0, Vorbis channel order for
/// family 1. The reserved families and family 255 define no channel meaning and are not
/// supported for playback.
pub fn channel_positions(channel_mapping_family: u8, channel_count: u8) -> Result<Channels> {
    let positions = match (channel_mapping_family, channel_count) {
        (CHANNEL_MAPPING_FAMILY_RTP | CHANNEL_MAPPING_FAMILY_VORBIS, 1) => Position::FRONT_LEFT,
        (CHANNEL_MAPPING_FAMILY_RTP | CHANNEL_MAPPING_FAMILY_VORBIS, 2) => {
            Position::FRONT_LEFT | Position::FRONT_RIGHT
        }
        (CHANNEL_MAPPING_FAMILY_RTP, _) => {
            return decode_error("common (opus): invalid channel count for mapping family 0");
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 3) => {
            Position::FRONT_LEFT | Position::FRONT_CENTER | Position::FRONT_RIGHT
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 4) => {
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::REAR_LEFT
                | Position::REAR_RIGHT
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 5) => {
            Position::FRONT_LEFT
                | Position::FRONT_CENTER
                | Position::FRONT_RIGHT
                | Position::REAR_LEFT
                | Position::REAR_RIGHT
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 6) => {
            Position::FRONT_LEFT
                | Position::FRONT_CENTER
                | Position::FRONT_RIGHT
                | Position::REAR_LEFT
                | Position::REAR_RIGHT
                | Position::LFE1
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 7) => {
            Position::FRONT_LEFT
                | Position::FRONT_CENTER
                | Position::FRONT_RIGHT
                | Position::SIDE_LEFT
                | Position::SIDE_RIGHT
                | Position::REAR_CENTER
                | Position::LFE1
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, 8) => {
            Position::FRONT_LEFT
                | Position::FRONT_CENTER
                | Position::FRONT_RIGHT
                | Position::SIDE_LEFT
                | Position::SIDE_RIGHT
                | Position::REAR_LEFT
                | Position::REAR_RIGHT
                | Position::LFE1
        }
        (CHANNEL_MAPPING_FAMILY_VORBIS, _) => {
            return decode_error("common (opus): invalid channel count for mapping family 1");
        }
        // Reserved, and should NOT be supported for playback.
        _ => return unsupported_error("common (opus): unsupported channel mapping family"),
    };

    Ok(Channels::Positioned(positions))
}

#[cfg(test)]
mod tests {
    use symphonia_core::io::BufReader;

    use super::*;

    const MAX_VERSION: u8 = 15;

    /// A family 0 stereo header: version 1, pre-skip 312, 48 kHz input, unity gain.
    fn stereo_head() -> OpusHead {
        OpusHead {
            version: OPUS_HEAD_VERSION,
            channel_count: 2,
            channels: Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT),
            original_sample_rate: 48_000,
            gain: 0,
            pre_skip: 312,
            channel_mapping_family: CHANNEL_MAPPING_FAMILY_RTP,
            channel_mapping_table: None,
        }
    }

    /// A family 1 5.1 header: 4 streams, 2 coupled, Vorbis order mapping.
    fn surround_head() -> OpusHead {
        OpusHead {
            channel_count: 6,
            channels: Channels::Positioned(
                Position::FRONT_LEFT
                    | Position::FRONT_CENTER
                    | Position::FRONT_RIGHT
                    | Position::REAR_LEFT
                    | Position::REAR_RIGHT
                    | Position::LFE1,
            ),
            gain: -256,
            channel_mapping_family: CHANNEL_MAPPING_FAMILY_VORBIS,
            channel_mapping_table: Some(ChannelMappingTable {
                stream_count: 4,
                coupled_count: 2,
                channel_mapping: Box::new([0, 4, 1, 2, 3, 5]),
            }),
            ..stereo_head()
        }
    }

    fn read(bytes: &[u8]) -> Result<OpusHead> {
        OpusHead::read(&mut BufReader::new(bytes), MAX_VERSION)
    }

    #[test]
    fn stereo_header_round_trips_through_its_canonical_bytes() {
        let bytes = stereo_head().to_bytes();
        assert_eq!(
            &*bytes,
            &[
                b'O', b'p', b'u', b's', b'H', b'e', b'a', b'd', 1, 2, 0x38, 0x01, 0x80, 0xbb, 0, 0,
                0, 0, 0
            ]
        );
        assert_eq!(read(&bytes).unwrap(), stereo_head());
    }

    #[test]
    fn surround_header_round_trips_with_its_mapping_table() {
        let bytes = surround_head().to_bytes();
        assert_eq!(bytes.len(), 19 + 2 + 6);
        assert_eq!(&bytes[19..], &[4, 2, 0, 4, 1, 2, 3, 5]);
        assert_eq!(read(&bytes).unwrap(), surround_head());
    }

    #[test]
    fn versions_above_the_maximum_are_rejected() {
        let mut bytes = stereo_head().to_bytes();
        bytes[8] = MAX_VERSION;
        assert!(read(&bytes).is_ok());
        bytes[8] = MAX_VERSION + 1;
        assert!(read(&bytes).is_err());
    }

    #[test]
    fn invalid_mapping_tables_are_rejected() {
        let valid = surround_head();
        let cases: [(u8, u8, &[u8]); 4] = [
            (0, 0, &[0, 1, 2, 3, 4, 5]),     // no streams
            (2, 3, &[0, 1, 2, 3, 4, 5]),     // more coupled streams than streams
            (200, 100, &[0, 1, 2, 3, 4, 5]), // N + M above 255
            (4, 2, &[0, 4, 1, 2, 3, 6]),     // entry names decoded channel 6 of 0..6
        ];
        for (stream_count, coupled_count, channel_mapping) in cases {
            let head = OpusHead {
                channel_mapping_table: Some(ChannelMappingTable {
                    stream_count,
                    coupled_count,
                    channel_mapping: Box::from(channel_mapping),
                }),
                ..valid.clone()
            };
            assert!(read(&head.to_bytes()).is_err(), "{stream_count} {coupled_count}");
        }
    }

    #[test]
    fn channel_counts_outside_the_family_range_are_rejected() {
        assert!(channel_positions(CHANNEL_MAPPING_FAMILY_RTP, 3).is_err());
        assert!(channel_positions(CHANNEL_MAPPING_FAMILY_VORBIS, 9).is_err());
        assert!(channel_positions(255, 2).is_err());
        assert_eq!(
            channel_positions(CHANNEL_MAPPING_FAMILY_VORBIS, 1).unwrap(),
            Channels::Positioned(Position::FRONT_LEFT)
        );
    }
}
