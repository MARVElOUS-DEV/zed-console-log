//! Track the ordered LSP queue so background confirmations cannot overtake edits.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};
use tokio::sync::Notify;
use tower_lsp_server::jsonrpc::Request;
use tower_service::Service;

#[derive(Default)]
pub struct Queue {
    received: AtomicU64,
    completed: AtomicU64,
    changed: Notify,
}

impl Queue {
    pub async fn drain(&self) {
        let target = self.received.load(Ordering::Acquire);
        loop {
            let changed = self.changed.notified();
            if self.completed.load(Ordering::Acquire) >= target {
                return;
            }
            changed.await;
        }
    }
}

pub struct Ordered<S> {
    pub inner: S,
    pub queue: Arc<Queue>,
}

impl<S> Service<Request> for Ordered<S>
where
    S: Service<Request>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let sequence = self.queue.received.fetch_add(1, Ordering::AcqRel) + 1;
        let future = self.inner.call(request);
        let queue = self.queue.clone();
        Box::pin(async move {
            let result = future.await;
            // Server concurrency must remain 1, preserving completion order.
            queue.completed.store(sequence, Ordering::Release);
            queue.changed.notify_one();
            result
        })
    }
}
