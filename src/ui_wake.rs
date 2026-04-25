use std::sync::Arc;

#[derive(Clone)]
pub struct UiWake(Arc<dyn Fn() + Send + Sync + 'static>);

impl UiWake {
    pub fn from_callback(callback: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }

    pub fn request_repaint(&self) {
        (self.0)();
    }
}
