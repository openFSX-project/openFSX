//! Optional game controller input. Left stick flies, right stick X is rudder,
//! triggers move the throttle, D-pad up/down moves flaps, A holds brakes,
//! X toggles the parking brake, Start pauses and Back resets.
use crate::desktop_flight::Analog;
use gilrs::{Axis, Button, EventType, Gilrs};
use winit::keyboard::KeyCode;

const DEAD_ZONE: f64 = 0.08;

pub struct Gamepad {
    gilrs: Option<Gilrs>,
}
impl Gamepad {
    pub fn new() -> Self {
        // A missing input backend leaves keyboard flight untouched.
        Self {
            gilrs: Gilrs::new().ok(),
        }
    }
    /// Drain button events as equivalent flight keys and sample the sticks of
    /// the first connected controller.
    pub fn poll(&mut self) -> (Vec<(KeyCode, bool)>, Option<Analog>) {
        let Some(gilrs) = &mut self.gilrs else {
            return (Vec::new(), None);
        };
        let mut keys = Vec::new();
        while let Some(event) = gilrs.next_event() {
            let (button, pressed) = match event.event {
                EventType::ButtonPressed(b, _) => (b, true),
                EventType::ButtonReleased(b, _) => (b, false),
                _ => continue,
            };
            let key = match button {
                Button::DPadUp => KeyCode::F6,
                Button::DPadDown => KeyCode::F7,
                Button::South => KeyCode::Space,
                Button::West => KeyCode::KeyB,
                Button::Start => KeyCode::KeyP,
                Button::Select => KeyCode::KeyR,
                _ => continue,
            };
            keys.push((key, pressed));
        }
        let analog = gilrs
            .gamepads()
            .find(|(_, g)| g.is_connected())
            .map(|(_, g)| {
                let axis = |a| shape(f64::from(g.value(a)));
                let trigger = |b| {
                    g.button_data(b)
                        .map_or(0., |d| f64::from(d.value()).clamp(0., 1.))
                };
                Analog {
                    pitch: -axis(Axis::LeftStickY),
                    roll: axis(Axis::LeftStickX),
                    yaw: axis(Axis::RightStickX),
                    throttle: None,
                    throttle_rate: trigger(Button::RightTrigger2) - trigger(Button::LeftTrigger2),
                    brake: 0.,
                }
            });
        (keys, analog)
    }
}
/// Dead zone with rescaling, then a mild curve for finer small corrections.
fn shape(v: f64) -> f64 {
    if !v.is_finite() || v.abs() <= DEAD_ZONE {
        return 0.;
    }
    let n = ((v.abs() - DEAD_ZONE) / (1. - DEAD_ZONE)).min(1.);
    v.signum() * (0.4 * n + 0.6 * n * n)
}
#[cfg(test)]
mod tests {
    #[test]
    fn dead_zone_and_curve_are_monotonic_and_bounded() {
        assert_eq!(super::shape(0.05), 0.);
        assert_eq!(super::shape(1.), 1.);
        assert_eq!(super::shape(-1.), -1.);
        assert!(super::shape(0.5) > super::shape(0.3) && super::shape(0.5) < 0.5);
        assert_eq!(super::shape(f64::NAN), 0.);
    }
}
