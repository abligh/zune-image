//! The color encoding an image header names: its primaries and its
//! transfer function, as the JPEG XL codestream's `ColorEncoding` bundle
//! (ISO/IEC 18181-1, `ColorEncoding`) holds them, with a D65 white
//! point and the relative rendering intent.

/// The primaries an RGB image's samples are in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JxlPrimaries {
    /// sRGB's, BT.709's.
    #[default]
    Srgb,
    /// ITU-R BT.2100's, BT.2020's.
    Bt2100,
    /// Display P3's: DCI-P3's primaries with a D65 white point.
    P3
}

/// How the samples code light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JxlTransferFunction {
    /// IEC 61966-2-1's sRGB curve.
    #[default]
    Srgb,
    /// ITU-R BT.709's (and BT.2020's) curve.
    Bt709,
    /// Linear light.
    Linear,
    /// SMPTE ST 2084's perceptual quantizer.
    Pq
}

/// An image's color encoding: sRGB by default, which the header writes
/// as `all_default`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JxlColorEncoding {
    /// The primaries, which a grayscale image leaves out.
    pub primaries:         JxlPrimaries,
    /// The transfer function.
    pub transfer_function: JxlTransferFunction
}

impl JxlPrimaries {
    /// The codestream's enum value.
    pub(crate) const fn value(self) -> u32 {
        match self {
            JxlPrimaries::Srgb => 1,
            JxlPrimaries::Bt2100 => 9,
            JxlPrimaries::P3 => 11
        }
    }
}

impl JxlTransferFunction {
    /// The codestream's enum value.
    pub(crate) const fn value(self) -> u32 {
        match self {
            JxlTransferFunction::Bt709 => 1,
            JxlTransferFunction::Linear => 8,
            JxlTransferFunction::Srgb => 13,
            JxlTransferFunction::Pq => 16
        }
    }
}

/// An enum value's bits as the codestream's `Enum` field codes it, a
/// `U32(Val(0), Val(1), BitsOffset(4, 2), BitsOffset(6, 18))`: the
/// selector's 2 bits, then any extra bits, each as `(count, value)`.
pub(crate) fn enum_bits(value: u32) -> [(u8, u64); 2] {
    match value {
        0 => [(2, 0), (0, 0)],
        1 => [(2, 1), (0, 0)],
        2..=17 => [(2, 2), (4, u64::from(value - 2))],
        _ => [(2, 3), (6, u64::from(value - 18))]
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use zune_core::bit_depth::BitDepth;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::EncoderOptions;

    use super::*;
    use crate::JxlSimpleEncoder;

    /// The codestream's bits, least significant first.
    struct Bits<'a> {
        data: &'a [u8],
        at:   usize
    }

    impl Bits<'_> {
        fn read(&mut self, count: usize) -> u32 {
            let mut value = 0;
            for i in 0..count {
                let bit = (self.data[self.at / 8] >> (self.at % 8)) & 1;
                value |= u32::from(bit) << i;
                self.at += 1;
            }
            value
        }

        fn read_enum(&mut self) -> u32 {
            match self.read(2) {
                0 => 0,
                1 => 1,
                2 => 2 + self.read(4),
                _ => 18 + self.read(6)
            }
        }
    }

    /// A 2 × 2 8-bit image of `colorspace` encoded with `encoding`,
    /// and its header read up to its color encoding: `None` where that
    /// is all_default, else its color space, white point, primaries
    /// (for RGB), transfer function and rendering intent.
    fn header(colorspace: ColorSpace, encoding: JxlColorEncoding) -> Option<Vec<u32>> {
        let channels = colorspace.num_components();
        let data = alloc::vec![100_u8; 4 * channels];
        let options = EncoderOptions::new(2, 2, colorspace, BitDepth::Eight);
        let mut encoder = JxlSimpleEncoder::new(&data, options);
        encoder.set_color_encoding(encoding);
        let mut out = Vec::new();
        encoder.encode(&mut out).unwrap();
        let mut bits = Bits { data: &out, at: 0 };
        assert_eq!(bits.read(16), 0x0AFF, "signature");
        assert_eq!(bits.read(1), 0, "not small");
        assert_eq!((bits.read(2), bits.read(9)), (0, 1), "height 2");
        assert_eq!(bits.read(3), 0, "no ratio");
        assert_eq!((bits.read(2), bits.read(9)), (0, 1), "width 2");
        assert_eq!(bits.read(1), 0, "metadata not all_default");
        assert_eq!(bits.read(1), 0, "no extra fields");
        assert_eq!(bits.read(1), 0, "integer samples");
        assert_eq!(bits.read(2), 0, "8 bits");
        assert_eq!(bits.read(1), 1, "16-bit buffer suffices");
        assert_eq!(bits.read(2), u32::from(colorspace.has_alpha()), "extra channels");
        if colorspace.has_alpha() {
            assert_eq!(bits.read(1), 1, "default alpha");
        }
        assert_eq!(bits.read(1), 0, "not xyb");
        if bits.read(1) == 1 {
            return None;
        }
        assert_eq!(bits.read(1), 0, "no ICC");
        let space = bits.read_enum();
        let mut fields = alloc::vec![space, bits.read_enum()];
        if space == 0 {
            fields.push(bits.read_enum());
        }
        assert_eq!(bits.read(1), 0, "no gamma");
        fields.push(bits.read_enum());
        fields.push(bits.read_enum());
        Some(fields)
    }

    #[test]
    fn srgb_is_all_default_and_others_are_named() {
        use JxlPrimaries as P;
        use JxlTransferFunction as T;
        for colorspace in [ColorSpace::RGB, ColorSpace::RGBA] {
            assert_eq!(header(colorspace, JxlColorEncoding::default()), None);
            for (primaries, p) in [(P::Srgb, 1), (P::Bt2100, 9), (P::P3, 11)] {
                for (transfer_function, t) in
                    [(T::Srgb, 13), (T::Bt709, 1), (T::Linear, 8), (T::Pq, 16)]
                {
                    let encoding = JxlColorEncoding {
                        primaries,
                        transfer_function
                    };
                    let want = (encoding != JxlColorEncoding::default())
                        .then(|| alloc::vec![0, 1, p, t, 1]);
                    assert_eq!(header(colorspace, encoding), want, "{encoding:?}");
                }
            }
        }
    }

    #[test]
    fn grayscale_names_its_transfer_and_no_primaries() {
        for (transfer_function, t) in [
            (JxlTransferFunction::Srgb, 13),
            (JxlTransferFunction::Bt709, 1),
            (JxlTransferFunction::Pq, 16)
        ] {
            let encoding = JxlColorEncoding {
                primaries: JxlPrimaries::P3,
                transfer_function
            };
            assert_eq!(
                header(ColorSpace::Luma, encoding),
                Some(alloc::vec![1, 1, t, 1])
            );
        }
    }

    /// The extra channel fields of a 2 × 2 RGBA image at `depth`'s
    /// header: `None` where it is all_default, else its type, its bits
    /// per sample, dim_shift, name length and premultiplication.
    fn alpha_info(depth: BitDepth) -> Option<[u32; 5]> {
        let data = alloc::vec![100_u8; 16 * depth.size_of()];
        let options = EncoderOptions::new(2, 2, ColorSpace::RGBA, depth);
        let mut out = Vec::new();
        JxlSimpleEncoder::new(&data, options)
            .encode(&mut out)
            .unwrap();
        let mut bits = Bits { data: &out, at: 0 };
        // Signature, size and the metadata up to its bit depth.
        bits.at = 16 + 1 + 11 + 3 + 11 + 1 + 1 + 1;
        if bits.read(2) == 3 {
            assert_eq!(bits.read(6) + 1, depth.bit_size() as u32, "bits");
        }
        bits.read(1);
        assert_eq!(bits.read(2), 1, "one extra channel");
        if bits.read(1) == 1 {
            return None;
        }
        let kind = bits.read_enum();
        assert_eq!(bits.read(1), 0, "integer samples");
        let per_sample = match bits.read(2) {
            0 => 8,
            1 => 10,
            2 => 12,
            _ => bits.read(6) + 1
        };
        Some([kind, per_sample, bits.read(2), bits.read(2), bits.read(1)])
    }

    #[test]
    fn alpha_is_as_deep_as_the_colors() {
        assert_eq!(alpha_info(BitDepth::Eight), None);
        assert_eq!(alpha_info(BitDepth::Sixteen), Some([0, 16, 0, 0, 0]));
    }

    /// A 2 × 2 8-bit RGB image in BT.2100 PQ, its intensity target
    /// `nits` where that is given, and its header read past its color
    /// encoding: its extra fields where it has them (orientation,
    /// intrinsic size, preview, animation) and its tone mapping fields
    /// (all_default, then intensity target, minimum nits, relative to
    /// the display's maximum and linear below), and the bits following.
    fn tone_mapping(nits: Option<f32>) -> (Option<[u32; 4]>, Option<[u32; 5]>, u32) {
        let data = alloc::vec![100_u8; 12];
        let options = EncoderOptions::new(2, 2, ColorSpace::RGB, BitDepth::Eight);
        let mut encoder = JxlSimpleEncoder::new(&data, options);
        encoder.set_color_encoding(JxlColorEncoding {
            primaries:         JxlPrimaries::Bt2100,
            transfer_function: JxlTransferFunction::Pq
        });
        if let Some(nits) = nits {
            encoder.set_intensity_target(nits);
        }
        let mut out = Vec::new();
        encoder.encode(&mut out).unwrap();
        let mut bits = Bits { data: &out, at: 0 };
        // Signature, size and the metadata's all_default.
        bits.at = 16 + 1 + 11 + 3 + 11;
        assert_eq!(bits.read(1), 0, "metadata not all_default");
        let extra = (bits.read(1) == 1)
            .then(|| [bits.read(3), bits.read(1), bits.read(1), bits.read(1)]);
        assert_eq!(bits.read(1), 0, "integer samples");
        assert_eq!(bits.read(2), 0, "8 bits");
        assert_eq!(bits.read(1), 1, "16-bit buffer suffices");
        assert_eq!(bits.read(2), 0, "no extra channels");
        assert_eq!(bits.read(1), 0, "not xyb");
        assert_eq!(bits.read(1), 0, "color encoding not all_default");
        assert_eq!(bits.read(1), 0, "no ICC");
        let named = [(); 3].map(|()| bits.read_enum());
        assert_eq!(named, [0, 1, 9], "RGB, D65, BT.2100");
        assert_eq!(bits.read(1), 0, "no gamma");
        assert_eq!([bits.read_enum(), bits.read_enum()], [16, 1], "PQ, relative");
        let fields = extra.map(|_| {
            [
                bits.read(1),
                bits.read(16),
                bits.read(16),
                bits.read(1),
                bits.read(16)
            ]
        });
        // No extensions, then the transform data's all_default.
        (extra, fields, bits.read(3))
    }

    #[test]
    fn an_intensity_target_other_than_255_is_written_in_the_tone_mapping() {
        // 10000 nits as a half float is 0x70E2.
        assert_eq!(
            tone_mapping(Some(10000.0)),
            (Some([0, 0, 0, 0]), Some([0, 0x70E2, 0, 0, 0]), 0b100)
        );
        assert_eq!(tone_mapping(Some(1000.0)).1, Some([0, 0x63D0, 0, 0, 0]));
        // 255, the default, given or not, is left unwritten.
        assert_eq!(tone_mapping(None), (None, None, 0b100));
        assert_eq!(tone_mapping(Some(255.0)), (None, None, 0b100));
    }

    #[test]
    fn an_enum_value_is_coded_by_its_range() {
        assert_eq!(enum_bits(0), [(2, 0), (0, 0)]);
        assert_eq!(enum_bits(1), [(2, 1), (0, 0)]);
        assert_eq!(enum_bits(13), [(2, 2), (4, 11)]);
        assert_eq!(enum_bits(17), [(2, 2), (4, 15)]);
        assert_eq!(enum_bits(18), [(2, 3), (6, 0)]);
    }
}
