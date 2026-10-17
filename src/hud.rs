//! Original vector/pixel flight overlay, independent of FSX gauges and fonts.
use crate::{
    aircraft::{FOOT, KNOT},
    flight::{Controls, Telemetry},
};

pub type Vertex = [f32; 6]; // clip x/y, linear RGBA
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Airborne,
    Paused,
    HighAlpha,
    Ground,
}
#[derive(Debug, Clone)]
pub struct Readout {
    pub knots: f64,
    pub feet: f64,
    pub fpm: f64,
    pub heading: f64,
    pub bank: f64,
    pub pitch: f64,
    pub throttle: f64,
    pub trim: f64,
    pub brake: f64,
    pub status: Status,
}
impl Readout {
    pub fn new(t: Telemetry, c: Controls, paused: bool) -> Result<Self, String> {
        let result = Self {
            knots: t.speed / KNOT,
            feet: t.altitude / FOOT,
            fpm: t.climb / FOOT * 60.,
            heading: t.attitude[2].to_degrees().rem_euclid(360.),
            bank: t.attitude[0].to_degrees(),
            pitch: t.attitude[1].to_degrees(),
            throttle: c.throttle * 100.,
            trim: c.trim * 100.,
            brake: c.brake * 100.,
            status: if paused {
                Status::Paused
            } else if t.ground_contact {
                Status::Ground
            } else if t.stalled {
                Status::HighAlpha
            } else {
                Status::Airborne
            },
        };
        if [
            result.knots,
            result.feet,
            result.fpm,
            result.heading,
            result.bank,
            result.pitch,
            result.throttle,
            result.trim,
            result.brake,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("Non-finite flight overlay state".into());
        }
        Ok(result)
    }
    pub fn rows(&self) -> [String; 8] {
        [
            format!("TAS  {:5.1} KT", self.knots),
            format!("ALT  {:5.0} FT", self.feet),
            format!("V/S {:+6.0} FPM", self.fpm),
            format!("HDG {:03.0} TRUE", self.heading.round().rem_euclid(360.)),
            format!("THR  {:5.0} %", self.throttle),
            format!("TRIM {:+5.0} %", self.trim),
            format!("BRK  {:5.0} %", self.brake),
            match self.status {
                Status::Airborne => "AIRBORNE",
                Status::Paused => "PAUSED - P RESUME",
                Status::HighAlpha => "POST-PEAK LIFT",
                Status::Ground => "ON WHEELS",
            }
            .into(),
        ]
    }
}
const WHITE: [f32; 4] = [0.86, 0.94, 1., 1.];
const BLUE: [f32; 4] = [0.15, 0.65, 1., 1.];
const GOLD: [f32; 4] = [1., 0.7, 0.16, 1.];
const PANEL: [f32; 4] = [0.008, 0.018, 0.034, 0.88];
struct Canvas {
    vertices: Vec<Vertex>,
    width: f32,
    height: f32,
}
impl Canvas {
    fn triangle(&mut self, points: [[f32; 2]; 3], color: [f32; 4]) {
        for [x, y] in points {
            self.vertices.push([
                x / self.width * 2. - 1.,
                1. - y / self.height * 2.,
                color[0],
                color[1],
                color[2],
                color[3],
            ]);
        }
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        self.triangle([[x, y], [x + w, y], [x + w, y + h]], c);
        self.triangle([[x, y], [x + w, y + h], [x, y + h]], c);
    }
    fn line(&mut self, a: [f32; 2], b: [f32; 2], thickness: f32, c: [f32; 4]) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy);
        if length < 1e-6 {
            return;
        }
        let n = [
            -dy / length * thickness * 0.5,
            dx / length * thickness * 0.5,
        ];
        let p = [
            [a[0] + n[0], a[1] + n[1]],
            [b[0] + n[0], b[1] + n[1]],
            [b[0] - n[0], b[1] - n[1]],
            [a[0] - n[0], a[1] - n[1]],
        ];
        self.triangle([p[0], p[1], p[2]], c);
        self.triangle([p[0], p[2], p[3]], c);
    }
    fn text(&mut self, x: f32, y: f32, text: &str, color: [f32; 4]) {
        for (i, ch) in text.chars().enumerate() {
            for (row, bits) in glyph(ch).into_iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        self.rect(
                            x + i as f32 * 12. + col as f32 * 2.,
                            y + row as f32 * 2.,
                            2.,
                            2.,
                            color,
                        );
                    }
                }
            }
        }
    }
}
// Five-by-seven original bitmap letterforms, encoded directly as row masks.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 14],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 27, 17],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        ':' => [0, 6, 6, 0, 6, 6, 0],
        _ => [0; 7],
    }
}
/// Pitch-up places the horizon below the aircraft symbol; right bank rotates
/// the world horizon counterclockwise in screen coordinates.
pub fn attitude_point(x: f32, angle: f32, bank: f32, pitch: f32) -> [f32; 2] {
    let y = (pitch - angle) * 2.;
    let r = bank.to_radians();
    [x * r.cos() + y * r.sin(), -x * r.sin() + y * r.cos()]
}
pub fn vertices(readout: &Readout, width: u32, height: u32) -> Vec<Vertex> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let scale = (width as f32 / 960.).min(height as f32 / 720.);
    let mut c = Canvas {
        vertices: Vec::new(),
        width: width as f32 / scale,
        height: height as f32 / scale,
    };
    c.rect(16., 16., 264., 252., PANEL);
    c.rect(16., 16., 4., 252., BLUE);
    c.text(32., 32., "OPENFSX / FLIGHT", BLUE);
    for (i, row) in readout.rows().iter().enumerate() {
        let color = if i == 7 && readout.status != Status::Airborne {
            GOLD
        } else {
            WHITE
        };
        c.text(32., 62. + i as f32 * 26., row, color);
    }
    let cx = c.width - 126.;
    let cy = 128.;
    c.rect(cx - 110., 16., 220., 252., PANEL);
    c.text(cx - 78., 32., "ATTITUDE", BLUE);
    for i in 0..48 {
        let a = i as f32 * std::f32::consts::TAU / 48.;
        let b = (i + 1) as f32 * std::f32::consts::TAU / 48.;
        c.line(
            [cx + a.cos() * 73., cy + a.sin() * 73.],
            [cx + b.cos() * 73., cy + b.sin() * 73.],
            1.,
            BLUE,
        );
    }
    for angle in [-30., -20., -10., 0., 10., 20., 30.] {
        let half = if angle == 0. { 65. } else { 26. };
        let a = attitude_point(-half, angle, readout.bank as f32, readout.pitch as f32);
        let b = attitude_point(half, angle, readout.bank as f32, readout.pitch as f32);
        // Clip to the circular indicator rather than leaking into its labels.
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let aa = dx * dx + dy * dy;
        let bb = 2. * (a[0] * dx + a[1] * dy);
        let cc = a[0] * a[0] + a[1] * a[1] - 71. * 71.;
        let discriminant = bb * bb - 4. * aa * cc;
        if discriminant < 0. {
            continue;
        }
        let t0 = ((-bb - discriminant.sqrt()) / (2. * aa)).max(0.);
        let t1 = ((-bb + discriminant.sqrt()) / (2. * aa)).min(1.);
        if t1 > t0 {
            c.line(
                [cx + a[0] + dx * t0, cy + a[1] + dy * t0],
                [cx + a[0] + dx * t1, cy + a[1] + dy * t1],
                if angle == 0. { 2. } else { 1. },
                WHITE,
            );
        }
    }
    c.line([cx - 32., cy], [cx - 10., cy], 3., GOLD);
    c.line([cx + 10., cy], [cx + 32., cy], 3., GOLD);
    c.line([cx - 10., cy], [cx, cy + 7.], 3., GOLD);
    c.line([cx, cy + 7.], [cx + 10., cy], 3., GOLD);
    c.text(
        cx - 96.,
        216.,
        &format!("BANK {:+6.1}", readout.bank),
        WHITE,
    );
    c.text(
        cx - 96.,
        242.,
        &format!("PITCH{:+6.1}", readout.pitch),
        WHITE,
    );
    c.rect(16., c.height - 94., c.width - 32., 78., PANEL);
    c.text(
        30.,
        c.height - 85.,
        "ARROWS: PITCH/BANK   Q/E: RUDDER   PGUP/PGDN: POWER",
        WHITE,
    );
    c.text(
        30.,
        c.height - 61.,
        "F1: IDLE   F4: FULL   B: PARK BRAKE   SPACE: BRAKES",
        WHITE,
    );
    c.text(
        30.,
        c.height - 37.,
        "HOME/END: TRIM   P: PAUSE   R: RESET   ESC: EXIT",
        WHITE,
    );
    c.vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    fn telemetry() -> Telemetry {
        Telemetry {
            speed: 90. * KNOT,
            altitude: 300.,
            alpha: 0.,
            beta: 0.,
            attitude: [0., 0., 0.],
            climb: 0.,
            stalled: false,
            ground_contact: false,
        }
    }
    #[test]
    fn displayed_units_heading_wrap_and_status_follow_simulation() {
        let mut t = telemetry();
        t.climb = FOOT * 5.;
        t.attitude[2] = -std::f64::consts::FRAC_PI_2;
        let controls = Controls {
            throttle: 0.5,
            trim: -0.25,
            ..Default::default()
        };
        let r = Readout::new(t, controls, false).unwrap();
        assert!((r.knots - 90.).abs() < 1e-9);
        assert!((r.feet - 984.2519685).abs() < 1e-6);
        assert!((r.fpm - 300.).abs() < 1e-9);
        assert_eq!(r.heading, 270.);
        assert_eq!(r.throttle, 50.);
        assert_eq!(r.trim, -25.);
        assert_eq!(r.rows()[0], "TAS   90.0 KT");
        assert_eq!(r.status, Status::Airborne);
        t.attitude[2] = 359.8_f64.to_radians();
        assert_eq!(
            Readout::new(t, controls, false).unwrap().rows()[3],
            "HDG 000 TRUE"
        );
        t.stalled = true;
        assert_eq!(
            Readout::new(t, controls, false).unwrap().status,
            Status::HighAlpha
        );
        assert_eq!(
            Readout::new(t, controls, true).unwrap().status,
            Status::Paused
        );
        t.ground_contact = true;
        assert_eq!(
            Readout::new(t, controls, true).unwrap().status,
            Status::Paused
        );
        t.altitude = f64::NAN;
        assert!(Readout::new(t, controls, false).is_err());
    }
    #[test]
    fn attitude_has_pilot_view_pitch_and_bank_signs() {
        assert_eq!(attitude_point(20., 0., 0., 0.), [20., 0.]);
        assert_eq!(attitude_point(0., 0., 0., 10.), [0., 20.]);
        assert!(attitude_point(50., 0., 30., 0.)[1] < 0.);
        assert!(attitude_point(-50., 0., 30., 0.)[1] > 0.);
    }
    #[test]
    fn resized_overlay_is_bounded_and_stays_within_gpu_capacity() {
        let r = Readout::new(telemetry(), Controls::default(), true).unwrap();
        for (w, h) in [(960, 720), (320, 200), (3840, 2160), (1100, 760)] {
            let mesh = vertices(&r, w, h);
            assert!(!mesh.is_empty() && mesh.len() < 65536);
            for v in mesh {
                assert!(v.iter().all(|f| f.is_finite()));
                assert!(v[0].abs() <= 1. && v[1].abs() <= 1.);
            }
        }
        assert!(vertices(&r, 0, 720).is_empty());
    }
}
