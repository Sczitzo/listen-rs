//! Windows controller choices use XInput and never consume another app's input.

const BUTTONS: [(&str, u16); 14] = [
    ("controller_dpad_up", 0x0001), ("controller_dpad_down", 0x0002),
    ("controller_dpad_left", 0x0004), ("controller_dpad_right", 0x0008),
    ("controller_start", 0x0010), ("controller_back", 0x0020),
    ("controller_left_stick", 0x0040), ("controller_right_stick", 0x0080),
    ("controller_left_bumper", 0x0100), ("controller_right_bumper", 0x0200),
    ("controller_a", 0x1000), ("controller_b", 0x2000),
    ("controller_x", 0x4000), ("controller_y", 0x8000),
];
const MASK: u16 = 0xF3FF;

pub fn binding(name: &str) -> Option<u16> {
    BUTTONS.iter().find(|(id, _)| *id == name).map(|(_, bit)| *bit)
}

#[cfg(windows)]
pub fn read() -> [Option<u16>; 4] {
    use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
    std::array::from_fn(|slot| {
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
        (unsafe { XInputGetState(slot as u32, &mut state) } == 0).then_some(state.Gamepad.wButtons & MASK)
    })
}

pub struct Capture {
    pub token: String,
    pub picked: Option<&'static str>,
    previous: [Option<u16>; 4],
    pressed: Option<(usize, u16)>,
}

impl Capture {
    pub fn new(token: String, previous: [Option<u16>; 4]) -> Self {
        Self { token, previous, pressed: None, picked: None }
    }

    pub fn poll(&mut self, buttons: [Option<u16>; 4]) -> bool {
        if self.picked.is_some() { return false; }
        if buttons.iter().flatten().count() != 1 {
            self.previous = [None; 4];
            self.pressed = None;
            return false;
        }
        if let Some((slot, bit)) = self.pressed {
            match buttons[slot] {
                Some(0) => self.picked = BUTTONS.iter().find(|(_, button)| *button == bit).map(|(id, _)| *id),
                Some(held) if held == bit => {},
                _ => self.pressed = None,
            }
        } else {
            self.pressed = buttons.iter().enumerate().find_map(|(slot, &now)| {
                now.filter(|bit| bit.count_ones() == 1 && self.previous[slot] == Some(0)).map(|bit| (slot, bit))
            });
        }
        self.previous = buttons;
        self.picked.is_some()
    }
}

#[derive(Default)]
pub struct Gesture {
    slot: Option<usize>,
    armed: bool,
    held: bool,
}

impl Gesture {
    pub fn poll(&mut self, pads: [Option<u16>; 4], selected: u16) -> (bool, bool) {
        let mut connected = pads.into_iter().enumerate().filter_map(|(slot, buttons)| buttons.map(|buttons| (slot, buttons)));
        let one = connected.next().filter(|_| connected.next().is_none());
        let previous_slot = self.slot;
        self.slot = one.map(|(slot, _)| slot);
        if previous_slot != self.slot || one.is_none() {
            self.held = false;
            self.armed = one.is_some_and(|(_, buttons)| buttons == 0);
            return (false, previous_slot.is_some());
        }
        let (_, buttons) = one.unwrap();
        match buttons {
            0 => {
                self.armed = true;
                self.held = false;
            }
            value if value == selected => {
                self.held |= self.armed;
                self.armed = false;
            }
            _ => {
                self.armed = false;
                let cancel = std::mem::take(&mut self.held);
                return (false, cancel);
            }
        }
        (self.held, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_requires_a_fresh_single_press_and_release_on_the_same_pad() {
        let mut capture = Capture::new("new attempt".into(), [Some(0x1000), Some(0), None, None]);
        assert!(!capture.poll([Some(0), Some(0), None, None]));
        assert!(!capture.poll([Some(0x3000), Some(0), None, None]));
        assert!(!capture.poll([None, Some(0x2000), None, None]));
        assert!(!capture.poll([None, None, None, None]));
        assert!(!capture.poll([Some(0), None, None, None]));
        assert!(!capture.poll([Some(0x1000), None, None, None]));
        assert!(capture.poll([Some(0), None, None, None]));
        assert_eq!(capture.picked, Some("controller_a"));
    }

    #[test]
    fn recording_requires_neutral_and_rejects_disconnect_or_side_buttons() {
        let mut state = Gesture::default();
        let selected = 0x1000;
        let poll = |state: &mut Gesture, button| state.poll([Some(button), None, None, None], selected);
        assert_eq!(poll(&mut state, selected), (false, false));
        assert_eq!(poll(&mut state, 0), (false, false));
        assert_eq!(poll(&mut state, selected), (true, false));
        assert_eq!(poll(&mut state, selected | 0x2000), (false, true));
        assert_eq!(poll(&mut state, selected), (false, false));
        poll(&mut state, 0);
        poll(&mut state, selected);
        assert_eq!(state.poll([None; 4], selected), (false, true));
        assert_eq!(poll(&mut state, selected), (false, false));
        poll(&mut state, 0);
        poll(&mut state, selected);
        assert_eq!(state.poll([Some(selected), Some(0), None, None], selected), (false, true));
    }
}
