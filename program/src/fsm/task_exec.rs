pub enum TaskExecutionState {
    ReceivingData,
    Executing,
    SendingResult,
}

pub struct TaskExecutionFSM {
    state: TaskExecutionState,
    task_id: u64,
    module_meta: ModuleMeta,
    params: Vec<Type>,
    received_data: Vec<u8>,
    received_chunks: BitVec,
}

impl StateMachine for TaskExecutionFSM {
    type Context = RuntimeContext;
    type Output = ();

    fn transition(&mut self, ctx: &mut Self::Context) -> Result<StateTransition<Self>, Error> {
        match self.state {
            TaskExecutionState::ReceivingData => {
                if self.received_chunks.all() {
                    self.state = TaskExecutionState::Executing;
                    Ok(StateTransition::Continue(self))
                } else {
                    ctx.request_chunks(self.missing_chunks());
                    Ok(StateTransition::Continue(self))
                }
            }
            TaskExecutionState::Executing => {
                let result = ctx.executor.execute(&self.received_data, &self.params)?;
                ctx.send_message(Message::ClientResult {
                    task_id: self.task_id,
                    result,
                })?;
                self.state = TaskExecutionState::SendingResult;
                Ok(StateTransition::Continue(self))
            }
            TaskExecutionState::SendingResult => {
                Ok(StateTransition::Complete(()))
            }
        }
    }
}