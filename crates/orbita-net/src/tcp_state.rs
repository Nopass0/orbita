//! TCP connection state machine (stage D.2, roadmap «Этап D»).
//!
//! Pure RFC-793 subset over a [`TcpControlBlock`]: every inbound segment
//! maps to one [`TcpAction`] (a segment to send, a lifecycle signal, or a
//! drop). No I/O, no timers, no allocation — the socket layer feeds
//! parsed segments in and executes the returned actions. The transition
//! matrix is host-tested («сегмент×состояние», see `tests` below).
//!
//! v1 scope (documented): no retransmission queue, no window
//! accounting beyond `rcv_nxt`, no options (MSS/WS), simultaneous-open
//! only for the SYN case, TIME_WAIT collapses on `timeout()`.

use crate::tcp::TcpFlags;

/// Connection lifecycle states (RFC 793 names).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

impl TcpState {
    /// Can the connection carry user data right now?
    pub fn can_send_data(self) -> bool {
        self == TcpState::Established || self == TcpState::CloseWait
    }

    /// Is the connection fully torn down?
    pub fn is_closed(self) -> bool {
        self == TcpState::Closed
    }
}

/// A segment the state machine wants transmitted.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SendPlan {
    pub flags: TcpFlags,
    pub sequence: u32,
    pub acknowledgment: u32,
}

/// What the socket layer should do after one inbound event.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TcpAction {
    /// Transmit this segment (checksum/payload assembly is the caller's).
    Send(SendPlan),
    /// The connection became ESTABLISHED (delivered to the app).
    Opened,
    /// The connection fully closed (FIN acked / RST / passive close done).
    Closed,
    /// Invalid or out-of-window segment — ignore silently (RFC: keep the
    /// connection untouched).
    Drop,
}

impl TcpAction {
    /// Convenience: an ACK-only send with the TCB's current counters.
    fn ack(cb: &TcpControlBlock, flags: TcpFlags) -> Self {
        TcpAction::Send(SendPlan {
            flags,
            sequence: cb.snd_nxt,
            acknowledgment: cb.rcv_nxt,
        })
    }
}

/// Connection control block: sequence counters + state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TcpControlBlock {
    pub state: TcpState,
    /// Our initial sequence number.
    pub snd_isn: u32,
    /// Next byte to send.
    pub snd_nxt: u32,
    /// Peer's initial sequence (valid after the first SYN).
    pub rcv_isn: u32,
    /// Next byte expected from the peer.
    pub rcv_nxt: u32,
}

impl TcpControlBlock {
    /// Passive open: a fresh LISTENing endpoint.
    pub fn listen() -> Self {
        Self {
            state: TcpState::Listen,
            snd_isn: 0,
            snd_nxt: 0,
            rcv_isn: 0,
            rcv_nxt: 0,
        }
    }

    /// Active open with our ISN: emits the SYN and enters SYN-SENT.
    pub fn active_open(isn: u32) -> (Self, TcpAction) {
        let cb = Self {
            state: TcpState::SynSent,
            snd_isn: isn,
            snd_nxt: isn.wrapping_add(1),
            rcv_isn: 0,
            rcv_nxt: 0,
        };
        let syn = TcpAction::Send(SendPlan {
            flags: TcpFlags::default().set(TcpFlags::SYN),
            sequence: isn,
            acknowledgment: 0,
        });
        (cb, syn)
    }

    /// `(sequence, acknowledgment)` for an outgoing data segment.
    pub fn send_header(&self) -> (u32, u32) {
        (self.snd_nxt, self.rcv_nxt)
    }

    /// Accounts `len` bytes of outgoing payload (after the caller built
    /// the segment via [`Self::send_header`]).
    pub fn data_sent(&mut self, len: usize) {
        self.snd_nxt = self.snd_nxt.wrapping_add(len as u32);
    }

    /// Feed one parsed inbound segment into the machine.
    ///
    /// `sequence`/`acknowledgment` are the header fields, `payload_len`
    /// the segment's data length (the payload itself belongs to the
    /// caller — only its length advances `rcv_nxt` here).
    pub fn on_segment(
        &mut self,
        sequence: u32,
        acknowledgment: u32,
        flags: TcpFlags,
        payload_len: usize,
    ) -> TcpAction {
        // RST tears down any connection (v1: no LISTEN↔SYN-RECEIVED nuance).
        if flags.has(TcpFlags::RST) && self.state != TcpState::Listen {
            self.state = TcpState::Closed;
            return TcpAction::Closed;
        }

        let seg_end = sequence.wrapping_add(payload_len as u32);

        match self.state {
            TcpState::Listen => {
                if flags.has(TcpFlags::SYN) {
                    self.rcv_isn = sequence;
                    self.rcv_nxt = sequence.wrapping_add(1);
                    self.snd_isn = self.snd_nxt;
                    self.state = TcpState::SynReceived;
                    return TcpAction::Send(SendPlan {
                        flags: TcpFlags::default().set(TcpFlags::SYN).set(TcpFlags::ACK),
                        sequence: self.snd_isn,
                        acknowledgment: self.rcv_nxt,
                    });
                }
                TcpAction::Drop
            }
            TcpState::SynSent => {
                if flags.has(TcpFlags::SYN) {
                    self.rcv_isn = sequence;
                    self.rcv_nxt = sequence.wrapping_add(1);
                    if flags.has(TcpFlags::ACK) {
                        // SYN+ACK completing our handshake: consume the ACK
                        // of our ISN and move straight to ESTABLISHED.
                        self.snd_nxt = acknowledgment;
                        self.state = TcpState::Established;
                        return TcpAction::Send(SendPlan {
                            flags: TcpFlags::default().set(TcpFlags::ACK),
                            sequence: self.snd_nxt,
                            acknowledgment: self.rcv_nxt,
                        });
                    }
                    // Simultaneous open: both sides sent SYN.
                    self.snd_isn = self.snd_nxt.wrapping_sub(1);
                    self.state = TcpState::SynReceived;
                    return TcpAction::Send(SendPlan {
                        flags: TcpFlags::default().set(TcpFlags::SYN).set(TcpFlags::ACK),
                        sequence: self.snd_isn,
                        acknowledgment: self.rcv_nxt,
                    });
                }
                TcpAction::Drop
            }
            TcpState::SynReceived => {
                if flags.has(TcpFlags::ACK) && acknowledgment == self.snd_isn.wrapping_add(1) {
                    self.state = TcpState::Established;
                    return TcpAction::Opened;
                }
                if flags.has(TcpFlags::SYN) {
                    // Retransmitted SYN: answer SYN+ACK again.
                    return TcpAction::Send(SendPlan {
                        flags: TcpFlags::default().set(TcpFlags::SYN).set(TcpFlags::ACK),
                        sequence: self.snd_isn,
                        acknowledgment: self.rcv_nxt,
                    });
                }
                TcpAction::Drop
            }
            TcpState::Established => {
                if sequence != self.rcv_nxt {
                    // Out-of-order or retransmission: re-ACK the expected.
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if flags.has(TcpFlags::FIN) {
                    self.rcv_nxt = seg_end.wrapping_add(1);
                    self.state = TcpState::CloseWait;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if payload_len > 0 {
                    self.rcv_nxt = seg_end;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if flags.has(TcpFlags::ACK) {
                    // Pure keep-alive/duplicate ACK: nothing to do.
                    return TcpAction::Drop;
                }
                TcpAction::Drop
            }
            TcpState::FinWait1 => {
                if flags.has(TcpFlags::FIN) && flags.has(TcpFlags::ACK) {
                    self.rcv_nxt = seg_end.wrapping_add(1);
                    self.state = TcpState::TimeWait;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if flags.has(TcpFlags::FIN) {
                    self.rcv_nxt = seg_end.wrapping_add(1);
                    self.state = TcpState::Closing;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if flags.has(TcpFlags::ACK) && acknowledgment == self.snd_nxt {
                    self.state = TcpState::FinWait2;
                    return TcpAction::Drop;
                }
                TcpAction::Drop
            }
            TcpState::FinWait2 => {
                if flags.has(TcpFlags::FIN) {
                    self.rcv_nxt = seg_end.wrapping_add(1);
                    self.state = TcpState::TimeWait;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                if payload_len > 0 && sequence == self.rcv_nxt {
                    self.rcv_nxt = seg_end;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                TcpAction::Drop
            }
            TcpState::CloseWait => {
                // Waiting for the local app to close(); data may still
                // arrive from the peer's side (half-close).
                if sequence == self.rcv_nxt && payload_len > 0 {
                    self.rcv_nxt = seg_end;
                    return TcpAction::ack(self, TcpFlags::default().set(TcpFlags::ACK));
                }
                TcpAction::Drop
            }
            TcpState::Closing => {
                if flags.has(TcpFlags::ACK) && acknowledgment == self.snd_nxt {
                    self.state = TcpState::TimeWait;
                    return TcpAction::Drop;
                }
                TcpAction::Drop
            }
            TcpState::LastAck => {
                if flags.has(TcpFlags::ACK) && acknowledgment == self.snd_nxt {
                    self.state = TcpState::Closed;
                    return TcpAction::Closed;
                }
                TcpAction::Drop
            }
            TcpState::TimeWait => TcpAction::Drop,
            TcpState::Closed => TcpAction::Drop,
        }
    }

    /// Active close (app called `close()`): FIN from ESTABLISHED or
    /// LAST-ACK transition from CLOSE-WAIT. The FIN consumes one sequence
    /// number.
    pub fn close(&mut self) -> TcpAction {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                let plan = SendPlan {
                    flags: TcpFlags::default().set(TcpFlags::FIN).set(TcpFlags::ACK),
                    sequence: self.snd_nxt,
                    acknowledgment: self.rcv_nxt,
                };
                self.snd_nxt = self.snd_nxt.wrapping_add(1);
                TcpAction::Send(plan)
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                let plan = SendPlan {
                    flags: TcpFlags::default().set(TcpFlags::FIN).set(TcpFlags::ACK),
                    sequence: self.snd_nxt,
                    acknowledgment: self.rcv_nxt,
                };
                self.snd_nxt = self.snd_nxt.wrapping_add(1);
                TcpAction::Send(plan)
            }
            _ => TcpAction::Drop,
        }
    }

    /// TIME_WAIT expiry (2*MSD): the socket layer calls this from its
    /// timer once the wait elapses.
    pub fn timeout(&mut self) -> TcpAction {
        if self.state == TcpState::TimeWait {
            self.state = TcpState::Closed;
            TcpAction::Closed
        } else {
            TcpAction::Drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYN: TcpFlags = TcpFlags(TcpFlags::SYN);
    const ACK: TcpFlags = TcpFlags(TcpFlags::ACK);
    const FIN: TcpFlags = TcpFlags(TcpFlags::FIN);
    const RST: TcpFlags = TcpFlags(TcpFlags::RST);

    fn syn_ack() -> TcpFlags {
        TcpFlags(TcpFlags::SYN | TcpFlags::ACK)
    }

    fn fin_ack() -> TcpFlags {
        TcpFlags(TcpFlags::FIN | TcpFlags::ACK)
    }

    fn send(flags: TcpFlags, seq: u32, ack: u32) -> TcpAction {
        TcpAction::Send(SendPlan { flags, sequence: seq, acknowledgment: ack })
    }

    // --- active open ------------------------------------------------------

    #[test]
    fn active_open_sends_syn_and_enters_syn_sent() {
        let (cb, action) = TcpControlBlock::active_open(1000);
        assert_eq!(cb.state, TcpState::SynSent);
        assert_eq!(action, send(SYN, 1000, 0));
        assert_eq!(cb.snd_nxt, 1001);
    }

    #[test]
    fn syn_sent_plus_syn_ack_establishes_and_acks() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        let action = cb.on_segment(5000, 1001, syn_ack(), 0);
        assert_eq!(cb.state, TcpState::Established);
        assert_eq!(action, send(ACK, 1001, 5001));
        assert_eq!(cb.rcv_nxt, 5001);
    }

    #[test]
    fn syn_sent_plus_bare_syn_is_simultaneous_open() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        let action = cb.on_segment(5000, 0, SYN, 0);
        assert_eq!(cb.state, TcpState::SynReceived);
        assert_eq!(action, send(syn_ack(), 1000, 5001));
    }

    #[test]
    fn syn_sent_plus_rst_closes() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        assert_eq!(cb.on_segment(0, 0, RST, 0), TcpAction::Closed);
        assert_eq!(cb.state, TcpState::Closed);
    }

    #[test]
    fn syn_sent_plus_random_data_drops() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        assert_eq!(cb.on_segment(77, 88, ACK, 5), TcpAction::Drop);
        assert_eq!(cb.state, TcpState::SynSent);
    }

    // --- passive open -----------------------------------------------------

    #[test]
    fn listen_plus_syn_answers_syn_ack() {
        let mut cb = TcpControlBlock::listen();
        let action = cb.on_segment(9000, 0, SYN, 0);
        assert_eq!(cb.state, TcpState::SynReceived);
        assert_eq!(action, send(syn_ack(), 0, 9001));
    }

    #[test]
    fn syn_received_plus_handshake_ack_establishes() {
        let mut cb = TcpControlBlock::listen();
        cb.on_segment(9000, 0, SYN, 0);
        assert_eq!(cb.on_segment(9001, 1, ACK, 0), TcpAction::Opened);
        assert_eq!(cb.state, TcpState::Established);
    }

    #[test]
    fn syn_received_plus_wrong_ack_drops() {
        let mut cb = TcpControlBlock::listen();
        cb.on_segment(9000, 0, SYN, 0);
        assert_eq!(cb.on_segment(9001, 42, ACK, 0), TcpAction::Drop);
        assert_eq!(cb.state, TcpState::SynReceived);
    }

    #[test]
    fn syn_received_plus_retransmitted_syn_reanswers() {
        let mut cb = TcpControlBlock::listen();
        cb.on_segment(9000, 0, SYN, 0);
        assert_eq!(cb.on_segment(9000, 0, SYN, 0), send(syn_ack(), 0, 9001));
    }

    // --- data path ---------------------------------------------------------

    #[test]
    fn established_data_advances_rcv_nxt_and_acks() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        let action = cb.on_segment(5001, 1001, ACK, 10);
        assert_eq!(action, send(ACK, 1001, 5011));
        assert_eq!(cb.rcv_nxt, 5011);
        cb.data_sent(10);
        assert_eq!(cb.send_header(), (1011, 5011));
    }

    #[test]
    fn established_out_of_order_data_reacks_expected() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        // Gap: peer sends seq 5011 before 5001..5011 arrived.
        let action = cb.on_segment(5011, 1001, ACK, 5);
        assert_eq!(action, send(ACK, 1001, 5001));
        assert_eq!(cb.rcv_nxt, 5001);
    }

    #[test]
    fn established_duplicate_pure_ack_drops() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        assert_eq!(cb.on_segment(5001, 1001, ACK, 0), TcpAction::Drop);
    }

    // --- teardown ----------------------------------------------------------

    #[test]
    fn established_plus_fin_enters_close_wait_and_acks() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        let action = cb.on_segment(5001, 1001, fin_ack(), 0);
        assert_eq!(cb.state, TcpState::CloseWait);
        assert_eq!(action, send(ACK, 1001, 5002));
    }

    #[test]
    fn close_wait_close_sends_fin_into_last_ack() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        cb.on_segment(5001, 1001, fin_ack(), 0);
        let action = cb.close();
        assert_eq!(cb.state, TcpState::LastAck);
        assert_eq!(action, send(fin_ack(), 1001, 5002));
    }

    #[test]
    fn last_ack_plus_final_ack_closes() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        cb.on_segment(5001, 1001, fin_ack(), 0);
        cb.close();
        assert_eq!(cb.on_segment(5002, 1002, ACK, 0), TcpAction::Closed);
        assert!(cb.state.is_closed());
    }

    #[test]
    fn active_close_fin_wait1_fin_ack_goes_time_wait_then_closed() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        let action = cb.close();
        assert_eq!(cb.state, TcpState::FinWait1);
        assert_eq!(action, send(fin_ack(), 1001, 5001));
        let action = cb.on_segment(5001, 1002, fin_ack(), 0);
        assert_eq!(cb.state, TcpState::TimeWait);
        assert_eq!(action, send(ACK, 1002, 5002));
        assert_eq!(cb.timeout(), TcpAction::Closed);
        assert!(cb.state.is_closed());
    }

    #[test]
    fn fin_wait1_bare_fin_is_simultaneous_close() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        cb.close();
        let action = cb.on_segment(5001, 1001, FIN, 0);
        assert_eq!(cb.state, TcpState::Closing);
        assert_eq!(action, send(ACK, 1002, 5002));
        let action = cb.on_segment(5002, 1002, ACK, 0);
        assert_eq!(cb.state, TcpState::TimeWait);
        assert_eq!(action, TcpAction::Drop);
        assert_eq!(cb.timeout(), TcpAction::Closed);
    }

    #[test]
    fn fin_wait2_data_still_flows_then_fin_closes() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        cb.close();
        cb.on_segment(5001, 1002, ACK, 0); // -> FIN-WAIT-2
        assert_eq!(cb.state, TcpState::FinWait2);
        let action = cb.on_segment(5001, 1002, ACK, 4);
        assert_eq!(action, send(ACK, 1002, 5005));
        let action = cb.on_segment(5005, 1002, FIN, 0);
        assert_eq!(cb.state, TcpState::TimeWait);
        assert_eq!(action, send(ACK, 1002, 5006));
        assert_eq!(cb.timeout(), TcpAction::Closed);
    }

    #[test]
    fn rst_from_any_state_closes() {
        let (mut cb, _) = TcpControlBlock::active_open(1000);
        cb.on_segment(5000, 1001, syn_ack(), 0);
        assert_eq!(cb.on_segment(0, 0, RST, 0), TcpAction::Closed);
        assert!(cb.state.is_closed());
    }

    #[test]
    fn closed_ignores_everything() {
        let mut cb = TcpControlBlock::listen();
        cb.state = TcpState::Closed;
        assert_eq!(cb.on_segment(1, 2, syn_ack(), 3), TcpAction::Drop);
        assert_eq!(cb.close(), TcpAction::Drop);
    }

    #[test]
    fn can_send_data_states() {
        assert!(TcpState::Established.can_send_data());
        assert!(TcpState::CloseWait.can_send_data());
        assert!(!TcpState::SynSent.can_send_data());
        assert!(!TcpState::FinWait2.can_send_data());
    }
}
