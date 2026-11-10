//! Asks a newly found vehicle for its home once, so the trace has a home marker without waiting
//! for a stack that never streams HOME_POSITION. This is a read request (like a parameter or
//! log-list request) and is sent on every link, physical ones included: it changes nothing on
//! the vehicle. MAV_CMD_REQUEST_MESSAGE is tried first; a vehicle that refuses it gets one
//! MAV_CMD_GET_HOME_POSITION (older firmware) and nothing more. SET_MESSAGE_INTERVAL is never
//! used: it would change the vehicle's stream configuration. A vehicle with no home yet may
//! answer later or not at all; until HOME_POSITION arrives the trace says so.
use super::frame::{Msg, dialect};

/// HOME_POSITION's message id.
pub const HOME_POSITION_ID: f32 = 242.0;

/// The one outstanding home request.
#[derive(Default)]
pub struct HomeRequest {
    /// `(system, component)` asked with REQUEST_MESSAGE whose answer is still awaited.
    awaiting: Option<(u8, u8)>,
}

impl HomeRequest {
    /// The request for a vehicle just found (`target` is its system and component).
    pub fn start(&mut self, target: (u8, u8)) -> Msg {
        self.awaiting = Some(target);
        let mut params = [0.0; 7];
        params[0] = HOME_POSITION_ID;
        command(dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE, params, target)
    }
    /// After a COMMAND_ACK addressed to us: the fallback request when REQUEST_MESSAGE was refused
    /// and the home is still unknown. Every other answer ends the request.
    pub fn on_ack(&mut self, ack: &dialect::COMMAND_ACK_DATA, home_known: bool) -> Option<Msg> {
        if ack.command != dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE {
            return None;
        }
        let target = self.awaiting.take()?;
        let refused = !matches!(
            ack.result,
            dialect::MavResult::MAV_RESULT_ACCEPTED | dialect::MavResult::MAV_RESULT_IN_PROGRESS
        );
        // Superseded by REQUEST_MESSAGE, but the only home request older firmware knows.
        #[allow(deprecated)]
        let fallback = dialect::MavCmd::MAV_CMD_GET_HOME_POSITION;
        (refused && !home_known).then(|| command(fallback, [0.0; 7], target))
    }
}

fn command(cmd: dialect::MavCmd, params: [f32; 7], (system, component): (u8, u8)) -> Msg {
    Msg::COMMAND_LONG(dialect::COMMAND_LONG_DATA {
        param1: params[0],
        param2: params[1],
        param3: params[2],
        param4: params[3],
        param5: params[4],
        param6: params[5],
        param7: params[6],
        command: cmd,
        target_system: system,
        target_component: component,
        confirmation: 0,
    })
}
