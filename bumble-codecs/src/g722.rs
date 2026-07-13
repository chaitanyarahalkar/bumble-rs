//! ITU-T G.722 64 kbit/s decoder.
//!
//! This is a stateful, integer-arithmetic port of `bumble.decoder.G722Decoder`.
//! One encoded byte produces two signed 16-bit little-endian PCM samples.

const WL: [i32; 8] = [-60, -30, 58, 172, 334, 538, 1198, 3042];
const RL42: [usize; 16] = [0, 7, 6, 5, 4, 3, 2, 1, 7, 6, 5, 4, 3, 2, 1, 0];
const ILB: [i32; 32] = [
    2048, 2093, 2139, 2186, 2233, 2282, 2332, 2383, 2435, 2489, 2543, 2599, 2656, 2714, 2774, 2834,
    2896, 2960, 3025, 3091, 3158, 3228, 3298, 3371, 3444, 3520, 3597, 3676, 3756, 3838, 3922, 4008,
];
const WH: [i32; 3] = [0, -214, 798];
const RH2: [usize; 4] = [2, 1, 2, 1];
const QM2: [i32; 4] = [-7408, -1616, 7408, 1616];
const QM4: [i32; 16] = [
    0, -20456, -12896, -8968, -6288, -4240, -2584, -1200, 20456, 12896, 8968, 6288, 4240, 2584,
    1200, 0,
];
const QM6: [i32; 64] = [
    -136, -136, -136, -136, -24808, -21904, -19008, -16704, -14984, -13512, -12280, -11192, -10232,
    -9360, -8576, -7856, -7192, -6576, -6000, -5456, -4944, -4464, -4008, -3576, -3168, -2776,
    -2400, -2032, -1688, -1360, -1040, -728, 24808, 21904, 19008, 16704, 14984, 13512, 12280,
    11192, 10232, 9360, 8576, 7856, 7192, 6576, 6000, 5456, 4944, 4464, 4008, 3576, 3168, 2776,
    2400, 2032, 1688, 1360, 1040, 728, 432, 136, -432, -136,
];
const QMF_COEFFS: [i32; 12] = [3, -11, 12, 32, -210, 951, 3876, -805, 362, -156, 53, -11];

#[derive(Clone, Debug)]
pub struct G722Decoder {
    x: [i32; 24],
    bands: [Band; 2],
}

impl Default for G722Decoder {
    fn default() -> Self {
        let mut bands = [Band::default(), Band::default()];
        bands[0].det = 32;
        bands[1].det = 8;
        Self { x: [0; 24], bands }
    }
}

impl G722Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode one frame into signed 16-bit little-endian PCM.
    pub fn decode_frame(&mut self, encoded_data: &[u8]) -> Vec<u8> {
        let samples = self.decode_samples(encoded_data);
        let mut pcm = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        pcm
    }

    /// Decode one frame into native signed PCM samples.
    pub fn decode_samples(&mut self, encoded_data: &[u8]) -> Vec<i16> {
        let mut samples = Vec::with_capacity(encoded_data.len() * 2);
        for code in encoded_data {
            let higher_bits = usize::from((code >> 6) & 0x03);
            let lower_bits = usize::from(code & 0x3F);
            let rlow = self.lower_sub_band_decoder(lower_bits);
            let rhigh = self.higher_sub_band_decoder(higher_bits);

            self.x.copy_within(2.., 0);
            self.x[22] = rlow + rhigh;
            self.x[23] = rlow - rhigh;

            let xout2: i32 = (0..12)
                .map(|index| self.x[2 * index] * QMF_COEFFS[index])
                .sum();
            let xout1: i32 = (0..12)
                .map(|index| self.x[2 * index + 1] * QMF_COEFFS[11 - index])
                .sum();
            samples.push((xout1 >> 11) as i16);
            samples.push((xout2 >> 11) as i16);
        }
        samples
    }

    fn lower_sub_band_decoder(&mut self, lower_bits: usize) -> i32 {
        let mut wd1 = lower_bits as i32;
        let mut wd2 = QM6[lower_bits];
        wd1 >>= 2;
        wd2 = (self.bands[0].det * wd2) >> 15;
        let rlow = (self.bands[0].s + wd2).clamp(-16384, 16383);

        let index = wd1 as usize;
        wd2 = QM4[index];
        let dlowt = (self.bands[0].det * wd2) >> 15;

        wd2 = RL42[index] as i32;
        wd1 = (self.bands[0].nb * 127) >> 7;
        wd1 += WL[wd2 as usize];
        self.bands[0].nb = wd1.clamp(0, 18432);

        wd1 = (self.bands[0].nb >> 6) & 31;
        wd2 = 8 - (self.bands[0].nb >> 11);
        let wd3 = if wd2 < 0 {
            ILB[wd1 as usize] << (-wd2 as u32)
        } else {
            ILB[wd1 as usize] >> (wd2 as u32)
        };
        self.bands[0].det = wd3 << 2;
        self.bands[0].block4(dlowt);
        rlow
    }

    fn higher_sub_band_decoder(&mut self, higher_bits: usize) -> i32 {
        let mut wd2 = QM2[higher_bits];
        let dhigh = (self.bands[1].det * wd2) >> 15;
        let rhigh = (dhigh + self.bands[1].s).clamp(-16384, 16383);

        wd2 = RH2[higher_bits] as i32;
        let mut wd1 = (self.bands[1].nb * 127) >> 7;
        wd1 += WH[wd2 as usize];
        self.bands[1].nb = wd1.clamp(0, 22528);

        wd1 = (self.bands[1].nb >> 6) & 31;
        wd2 = 10 - (self.bands[1].nb >> 11);
        let wd3 = if wd2 < 0 {
            ILB[wd1 as usize] << (-wd2 as u32)
        } else {
            ILB[wd1 as usize] >> (wd2 as u32)
        };
        self.bands[1].det = wd3 << 2;
        self.bands[1].block4(dhigh);
        rhigh
    }
}

#[derive(Clone, Debug, Default)]
struct Band {
    s: i32,
    nb: i32,
    det: i32,
    sp: i32,
    sz: i32,
    r: [i32; 3],
    a: [i32; 3],
    ap: [i32; 3],
    p: [i32; 3],
    d: [i32; 7],
    b: [i32; 7],
    bp: [i32; 7],
    sg: [i32; 7],
}

impl Band {
    fn saturate(value: i32) -> i32 {
        value.clamp(-32768, 32767)
    }

    fn block4(&mut self, d: i32) {
        self.d[0] = d;
        self.r[0] = Self::saturate(self.s + d);
        self.p[0] = Self::saturate(self.sz + d);

        for index in 0..3 {
            self.sg[index] = self.p[index] >> 15;
        }
        let wd1 = Self::saturate(self.a[1] << 2);
        let mut wd2 = if self.sg[0] == self.sg[1] { -wd1 } else { wd1 };
        if wd2 > 32767 {
            wd2 = 32767;
        }
        let mut wd3 = if self.sg[0] == self.sg[2] { 128 } else { -128 };
        wd3 += wd2 >> 7;
        wd3 += (self.a[2] * 32512) >> 15;
        self.ap[2] = wd3.clamp(-12288, 12288);

        self.sg[0] = self.p[0] >> 15;
        self.sg[1] = self.p[1] >> 15;
        let wd1 = if self.sg[0] == self.sg[1] { 192 } else { -192 };
        wd2 = (self.a[1] * 32640) >> 15;
        self.ap[1] = Self::saturate(wd1 + wd2);
        wd3 = Self::saturate(15360 - self.ap[2]);
        self.ap[1] = self.ap[1].clamp(-wd3, wd3);

        let wd1 = if d == 0 { 0 } else { 128 };
        self.sg[0] = d >> 15;
        for index in 1..7 {
            self.sg[index] = self.d[index] >> 15;
            wd2 = if self.sg[index] == self.sg[0] {
                wd1
            } else {
                -wd1
            };
            wd3 = (self.b[index] * 32640) >> 15;
            self.bp[index] = Self::saturate(wd2 + wd3);
        }

        for index in (1..7).rev() {
            self.d[index] = self.d[index - 1];
            self.b[index] = self.bp[index];
        }
        for index in (1..3).rev() {
            self.r[index] = self.r[index - 1];
            self.p[index] = self.p[index - 1];
            self.a[index] = self.ap[index];
        }

        self.sp = 0;
        for index in 1..3 {
            let doubled = Self::saturate(self.r[index] + self.r[index]);
            self.sp += (self.a[index] * doubled) >> 15;
        }
        self.sp = Self::saturate(self.sp);

        self.sz = 0;
        for index in (1..7).rev() {
            let doubled = Self::saturate(self.d[index] + self.d[index]);
            self.sz += (self.b[index] * doubled) >> 15;
        }
        self.sz = Self::saturate(self.sz);
        self.s = Self::saturate(self.sp + self.sz);
    }
}
