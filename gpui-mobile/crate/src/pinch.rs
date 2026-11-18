//! Two-finger pinch to zoom, recognized from raw contacts.
//!
//! GPUI's portable touch recognizer (gpui-pre 0.3.12) handles one contact at
//! a time: it has no pinch, and ignores a second finger while the first is
//! recognized. The platforms deliver every contact, so this recognizer sits
//! between them and GPUI and turns two fingers into
//! [`PinchEvent`]s (surfaced to elements as `on_pinch`, as a trackpad's
//! pinch is on a desktop).
//!
//! One finger goes on to GPUI untouched (taps, long presses, drags, pans).
//! When a second lands, the first is cancelled in GPUI (a pan stops where it
//! is, a pending tap or long press never fires) and the two fingers pinch:
//! `Started` at their midpoint, then `Moved` with how much the distance
//! between them grew since the last step, and `Ended` when either lifts. The
//! contacts still down after that, and any third finger, are not GPUI's:
//! they are ignored until every finger has lifted, so a pinch never turns
//! into a tap or a scroll of the finger left behind.
//!
//! Main thread only, like everything in GPUI.

use gpui::{Modifiers, PinchEvent, Pixels, Point, TouchEvent, TouchId, TouchPhase};

/// What to hand GPUI for one raw contact event.
#[derive(Debug)]
pub(crate) enum Relay {
    /// A contact for GPUI's own recognizer.
    Touch(TouchEvent),
    Pinch(PinchEvent),
}

enum State {
    /// At most one finger, which is GPUI's.
    Idle,
    /// Two fingers pinching, this far apart at the last step.
    Pinching { span: f32 },
    /// A pinch ended with fingers still down: they lift unseen.
    Draining,
}

pub(crate) struct PinchRecognizer {
    /// The fingers down, in the order they landed, and where each is.
    contacts: Vec<(TouchId, Point<Pixels>)>,
    state: State,
}

impl PinchRecognizer {
    pub(crate) fn new() -> Self {
        Self {
            contacts: Vec::new(),
            state: State::Idle,
        }
    }

    /// Takes one raw contact event and returns what GPUI gets for it, in
    /// order.
    pub(crate) fn handle(&mut self, event: TouchEvent) -> Vec<Relay> {
        let mut out = Vec::new();
        match event.phase {
            TouchPhase::Started => {
                self.contacts.push((event.id, event.position));
                match self.state {
                    State::Idle if self.contacts.len() == 1 => out.push(Relay::Touch(event)),
                    State::Idle => {
                        // The second finger: the first stops being GPUI's.
                        let (first, at) = self.contacts[0];
                        out.push(Relay::Touch(TouchEvent {
                            id: first,
                            phase: TouchPhase::Cancelled,
                            position: at,
                            predicted_position: None,
                            force: None,
                        }));
                        self.state = State::Pinching { span: self.span() };
                        out.push(self.pinch(TouchPhase::Started, 0.));
                    }
                    State::Pinching { .. } | State::Draining => {}
                }
            }
            TouchPhase::Moved => {
                if let Some(contact) = self.contacts.iter_mut().find(|(id, _)| *id == event.id) {
                    contact.1 = event.position;
                }
                match self.state {
                    State::Idle => out.push(Relay::Touch(event)),
                    State::Pinching { span } => {
                        let now = self.span();
                        // Only the first two fingers pinch; a third moving
                        // changes nothing.
                        if span > 0. && now != span {
                            self.state = State::Pinching { span: now };
                            out.push(self.pinch(TouchPhase::Moved, now / span - 1.));
                        }
                    }
                    State::Draining => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let was = self.contacts.iter().position(|(id, _)| *id == event.id);
                match self.state {
                    State::Idle => {
                        if was.is_some() {
                            out.push(Relay::Touch(event));
                        }
                    }
                    State::Pinching { .. } if was.is_some_and(|i| i < 2) => {
                        out.push(self.pinch(TouchPhase::Ended, 0.));
                        self.state = State::Draining;
                    }
                    State::Pinching { .. } | State::Draining => {}
                }
                if let Some(i) = was {
                    self.contacts.remove(i);
                }
                if self.contacts.is_empty() {
                    self.state = State::Idle;
                }
            }
        }
        out
    }

    /// How far apart the two pinching fingers are.
    fn span(&self) -> f32 {
        match &self.contacts[..] {
            [(_, a), (_, b), ..] => (*a - *b).magnitude() as f32,
            _ => 0.,
        }
    }

    fn pinch(&self, phase: TouchPhase, delta: f32) -> Relay {
        let position = match &self.contacts[..] {
            [(_, a), (_, b), ..] => Point::new((a.x + b.x) / 2., (a.y + b.y) / 2.),
            [(_, a)] => *a,
            [] => Point::default(),
        };
        Relay::Pinch(PinchEvent {
            position,
            delta,
            modifiers: Modifiers::default(),
            phase,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> TouchEvent {
        TouchEvent {
            id: TouchId(id),
            phase,
            position: point(px(x), px(y)),
            predicted_position: None,
            force: None,
        }
    }

    fn kinds(relays: &[Relay]) -> Vec<String> {
        relays
            .iter()
            .map(|r| match r {
                Relay::Touch(t) => format!("touch {} {:?}", t.id.0, t.phase),
                Relay::Pinch(p) => format!("pinch {:?}", p.phase),
            })
            .collect()
    }

    #[test]
    fn one_finger_goes_to_gpui_untouched() {
        let mut r = PinchRecognizer::new();
        for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
            assert_eq!(kinds(&r.handle(touch(1, phase, 10., 10.))), [format!("touch 1 {phase:?}")]);
        }
    }

    #[test]
    fn two_fingers_pinch_and_the_first_is_cancelled_in_gpui() {
        let mut r = PinchRecognizer::new();
        r.handle(touch(1, TouchPhase::Started, 100., 100.));
        r.handle(touch(1, TouchPhase::Moved, 100., 110.));
        let second = r.handle(touch(2, TouchPhase::Started, 100., 210.));
        assert_eq!(kinds(&second), ["touch 1 Cancelled", "pinch Started"]);
        let Relay::Pinch(started) = &second[1] else { unreachable!() };
        assert_eq!(started.position, point(px(100.), px(160.)));
        // 100 apart, then 150: half again as far.
        let moved = r.handle(touch(2, TouchPhase::Moved, 100., 260.));
        let [Relay::Pinch(moved)] = &moved[..] else { panic!("{moved:?}") };
        assert_eq!(moved.phase, TouchPhase::Moved);
        assert!((moved.delta - 0.5).abs() < 1e-5, "{}", moved.delta);
        // Back to 75 apart: half as far as the last step.
        let moved = r.handle(touch(1, TouchPhase::Moved, 100., 185.));
        let [Relay::Pinch(moved)] = &moved[..] else { panic!("{moved:?}") };
        assert!((moved.delta + 0.5).abs() < 1e-5, "{}", moved.delta);
        // Either lifting ends it; the other lifts unseen.
        assert_eq!(kinds(&r.handle(touch(1, TouchPhase::Ended, 100., 185.))), ["pinch Ended"]);
        assert!(r.handle(touch(2, TouchPhase::Moved, 140., 300.)).is_empty());
        assert!(r.handle(touch(2, TouchPhase::Ended, 140., 300.)).is_empty());
        // Then one finger is GPUI's again.
        assert_eq!(kinds(&r.handle(touch(3, TouchPhase::Started, 5., 5.))), ["touch 3 Started"]);
    }

    #[test]
    fn a_third_finger_is_ignored() {
        let mut r = PinchRecognizer::new();
        r.handle(touch(1, TouchPhase::Started, 0., 0.));
        r.handle(touch(2, TouchPhase::Started, 0., 100.));
        assert!(r.handle(touch(3, TouchPhase::Started, 50., 50.)).is_empty());
        assert!(r.handle(touch(3, TouchPhase::Moved, 90., 90.)).is_empty());
        assert!(r.handle(touch(3, TouchPhase::Ended, 90., 90.)).is_empty());
        // The pinch goes on with the first two.
        assert_eq!(kinds(&r.handle(touch(2, TouchPhase::Moved, 0., 200.))), ["pinch Moved"]);
    }
}
