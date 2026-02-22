use rust_fsm::*;

state_machine! {
    inbox_flow(New)

    New(HydrateAcknowledged) => Acknowledged,
    New(HydrateInProgress) => InProgress,
    New(HydrateBlocked) => Blocked,
    New(HydrateDone) => Done,
    New(HydrateDismissed) => Dismissed,

    New(Acknowledge) => Acknowledged,
    New(Start) => InProgress,
    New(Block) => Blocked,
    New(DoneEvent) => Done,
    New(Dismiss) => Dismissed,
    New(Snooze) => New,

    Acknowledged(Start) => InProgress,
    Acknowledged(Block) => Blocked,
    Acknowledged(DoneEvent) => Done,
    Acknowledged(Dismiss) => Dismissed,
    Acknowledged(Snooze) => New,

    InProgress(Block) => Blocked,
    InProgress(DoneEvent) => Done,
    InProgress(Dismiss) => Dismissed,
    InProgress(Snooze) => New,

    Blocked(Start) => InProgress,
    Blocked(DoneEvent) => Done,
    Blocked(Dismiss) => Dismissed,
    Blocked(Snooze) => New,

    Done(Reopen) => InProgress
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxState {
    New,
    Acknowledged,
    InProgress,
    Blocked,
    Done,
    Dismissed,
}

impl InboxState {
    pub fn is_actionable(self) -> bool {
        matches!(
            self,
            InboxState::New
                | InboxState::Acknowledged
                | InboxState::InProgress
                | InboxState::Blocked
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxAction {
    Acknowledge,
    Start,
    Block,
    Done,
    Reopen,
    Dismiss,
    Snooze,
}

fn hydrate(machine: &mut inbox_flow::StateMachine, state: InboxState) -> Result<(), ()> {
    let input = match state {
        InboxState::New => return Ok(()),
        InboxState::Acknowledged => inbox_flow::Input::HydrateAcknowledged,
        InboxState::InProgress => inbox_flow::Input::HydrateInProgress,
        InboxState::Blocked => inbox_flow::Input::HydrateBlocked,
        InboxState::Done => inbox_flow::Input::HydrateDone,
        InboxState::Dismissed => inbox_flow::Input::HydrateDismissed,
    };
    machine.consume(&input).map_err(|_| ())?;
    Ok(())
}

fn expected_next_state(current: InboxState, action: InboxAction) -> Option<InboxState> {
    match (current, action) {
        (InboxState::New, InboxAction::Acknowledge) => Some(InboxState::Acknowledged),
        (InboxState::New, InboxAction::Start) => Some(InboxState::InProgress),
        (InboxState::New, InboxAction::Block) => Some(InboxState::Blocked),
        (InboxState::New, InboxAction::Done) => Some(InboxState::Done),
        (InboxState::New, InboxAction::Dismiss) => Some(InboxState::Dismissed),
        (InboxState::New, InboxAction::Snooze) => Some(InboxState::New),
        (InboxState::Acknowledged, InboxAction::Start) => Some(InboxState::InProgress),
        (InboxState::Acknowledged, InboxAction::Block) => Some(InboxState::Blocked),
        (InboxState::Acknowledged, InboxAction::Done) => Some(InboxState::Done),
        (InboxState::Acknowledged, InboxAction::Dismiss) => Some(InboxState::Dismissed),
        (InboxState::Acknowledged, InboxAction::Snooze) => Some(InboxState::New),
        (InboxState::InProgress, InboxAction::Block) => Some(InboxState::Blocked),
        (InboxState::InProgress, InboxAction::Done) => Some(InboxState::Done),
        (InboxState::InProgress, InboxAction::Dismiss) => Some(InboxState::Dismissed),
        (InboxState::InProgress, InboxAction::Snooze) => Some(InboxState::New),
        (InboxState::Blocked, InboxAction::Start) => Some(InboxState::InProgress),
        (InboxState::Blocked, InboxAction::Done) => Some(InboxState::Done),
        (InboxState::Blocked, InboxAction::Dismiss) => Some(InboxState::Dismissed),
        (InboxState::Blocked, InboxAction::Snooze) => Some(InboxState::New),
        (InboxState::Done, InboxAction::Reopen) => Some(InboxState::InProgress),
        _ => None,
    }
}

pub fn transition(current: InboxState, action: InboxAction) -> Option<InboxState> {
    let mut machine = inbox_flow::StateMachine::new();
    hydrate(&mut machine, current).ok()?;

    let input = match action {
        InboxAction::Acknowledge => inbox_flow::Input::Acknowledge,
        InboxAction::Start => inbox_flow::Input::Start,
        InboxAction::Block => inbox_flow::Input::Block,
        InboxAction::Done => inbox_flow::Input::DoneEvent,
        InboxAction::Reopen => inbox_flow::Input::Reopen,
        InboxAction::Dismiss => inbox_flow::Input::Dismiss,
        InboxAction::Snooze => inbox_flow::Input::Snooze,
    };

    machine.consume(&input).ok()?;
    expected_next_state(current, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_fsm_allows_happy_path() {
        assert_eq!(
            transition(InboxState::New, InboxAction::Acknowledge),
            Some(InboxState::Acknowledged)
        );
        assert_eq!(
            transition(InboxState::Acknowledged, InboxAction::Start),
            Some(InboxState::InProgress)
        );
        assert_eq!(
            transition(InboxState::InProgress, InboxAction::Done),
            Some(InboxState::Done)
        );
    }

    #[test]
    fn inbox_fsm_rejects_invalid_transition() {
        assert_eq!(transition(InboxState::Done, InboxAction::Start), None);
        assert_eq!(transition(InboxState::Dismissed, InboxAction::Done), None);
    }
}
