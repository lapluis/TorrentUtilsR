/// Receives piece-processing progress without coupling the core library to a UI.
pub trait ProgressReporter: Sync {
    fn begin(&self, total: usize);
    fn advance(&self, delta: usize);
    fn finish(&self);
}
