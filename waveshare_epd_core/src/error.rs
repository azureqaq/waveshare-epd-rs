use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    TimeOut(#[from] TimeOutError),
}

#[derive(Debug, thiserror::Error)]
#[error("timeout: {:?}, elapsed: {:?}", self.timeout, self.elapsed)]
pub struct TimeOutError {
    pub timeout: Duration,
    pub elapsed: Duration,
}
