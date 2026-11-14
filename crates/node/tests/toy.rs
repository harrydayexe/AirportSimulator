//! A toy node, to show the [`Node`] trait can be implemented outside the
//! crate and behaves as the host expects.
use det::{Ms, NodeId};
use node::{Effect, Input, Node};

/// Pings a peer every `period` and answers any ping with a pong.
struct Pinger {
    id: NodeId,
    peer: NodeId,
    period: Ms,
    due: Ms,
}

#[derive(Debug, PartialEq, Eq)]
enum Ping {
    Ping,
    Pong,
}

impl Node for Pinger {
    type Message = Ping;

    fn id(&self) -> NodeId {
        self.id
    }

    fn step(&mut self, now: Ms, input: Input<Ping>) -> Vec<Effect<Ping>> {
        match input {
            // A wake may arrive early, so check the time ourselves.
            Input::Wake if now < self.due => vec![],
            Input::Wake => {
                self.due = Ms(now.0 + self.period.0);
                vec![Effect::Send {
                    to: self.peer,
                    message: Ping::Ping,
                }]
            }
            Input::Message {
                from,
                message: Ping::Ping,
            } => vec![Effect::Send {
                to: from,
                message: Ping::Pong,
            }],
            Input::Message {
                message: Ping::Pong,
                ..
            } => vec![],
        }
    }

    fn next_wake(&self) -> Option<Ms> {
        Some(self.due)
    }
}

fn pinger() -> Pinger {
    Pinger {
        id: NodeId(1),
        peer: NodeId(2),
        period: Ms(100),
        due: Ms(100),
    }
}

#[test]
fn early_wake_does_nothing() {
    let mut n = pinger();
    assert_eq!(n.step(Ms(50), Input::Wake), vec![]);
    assert_eq!(n.next_wake(), Some(Ms(100)));
}

#[test]
fn due_or_late_wake_pings_the_peer_and_reschedules() {
    let mut n = pinger();
    assert_eq!(
        n.step(Ms(130), Input::Wake),
        vec![Effect::Send {
            to: NodeId(2),
            message: Ping::Ping
        }]
    );
    assert_eq!(n.next_wake(), Some(Ms(230)));
}

#[test]
fn answers_a_ping_with_a_pong() {
    let mut n = pinger();
    let effects = n.step(
        Ms(10),
        Input::Message {
            from: NodeId(3),
            message: Ping::Ping,
        },
    );
    assert_eq!(
        effects,
        vec![Effect::Send {
            to: NodeId(3),
            message: Ping::Pong
        }]
    );
    assert_eq!(n.id(), NodeId(1));
}

#[test]
fn next_wake_is_a_pure_read() {
    let n = pinger();
    assert_eq!(n.next_wake(), n.next_wake());
}
