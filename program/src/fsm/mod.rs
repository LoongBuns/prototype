pub trait StateMachine {
    type Context;
    type Output;
    
    fn transition(
        &mut self,
        ctx: &mut Self::Context
    ) -> Result<StateTransition<Self>, Error>;
}

pub enum StateTransition<S: StateMachine> {
    Continue(S),
    Complete(S::Output),
    Error(Error),
}