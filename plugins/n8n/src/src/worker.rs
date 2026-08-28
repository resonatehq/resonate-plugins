use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use resonate_core::types::{
    Message, RequestEnvelope, RequestHead, TaskAcquireResponseData, PROTOCOL_VERSION,
};
use resonate_core::{ResonateServer, ResonateWorker, Unavailable};

const PID: &str = "self";

pub struct Worker {
    server: Arc<dyn ResonateServer>,
    config: crate::plugin::Config,
    lease_timeout: i64,
}

impl Worker {
    pub fn new(
        server: Arc<dyn ResonateServer>,
        config: crate::plugin::Config,
        lease_timeout: i64,
    ) -> Self {
        Self { server, config, lease_timeout }
    }
}

#[async_trait]
impl ResonateWorker for Worker {
    async fn send(&self, _address: &str, msg: &Message) -> Result<(), Unavailable> {
        let task = match msg {
            Message::Execute(e) => &e.data.task,
            Message::Unblock(_) => return Ok(()),
        };
        let ctx = RunContext {
            server: Arc::clone(&self.server),
            config: self.config.clone(),
            lease_timeout: self.lease_timeout,
            task_id: task.id.clone(),
            task_version: task.version,
        };
        tokio::spawn(async move { ctx.run().await });
        Ok(())
    }
}

struct RunContext {
    server: Arc<dyn ResonateServer>,
    config: crate::plugin::Config,
    lease_timeout: i64,
    task_id: String,
    task_version: i64,
}

impl RunContext {
    async fn run(self) {
        let Ok(claimed) = self
            .server
            .process(&RequestEnvelope {
                kind: "task.acquire".into(),
                head: head(),
                data: json!({
                    "id": self.task_id,
                    "version": self.task_version,
                    "pid": PID,
                    "ttl": self.lease_timeout,
                }),
            })
            .await
        else {
            return;
        };
        if claimed.head.status != 200 {
            return;
        }
        let Ok(acquired) = serde_json::from_value::<TaskAcquireResponseData>(claimed.data) else {
            return;
        };
        let version = acquired.task.version;
        let promise = acquired.promise;

        let heartbeat = {
            let server = Arc::clone(&self.server);
            let task_id = self.task_id.clone();
            let beat = Duration::from_millis((self.lease_timeout / 3).max(1) as u64);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(beat);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let _ = server
                        .process(&RequestEnvelope {
                            kind: "task.heartbeat".into(),
                            head: head(),
                            data: json!({
                                "pid": PID,
                                "tasks": [{ "id": task_id, "version": version }]
                            }),
                        })
                        .await;
                }
            })
        };
        let verdict = crate::plugin::process(&self.config, &promise).await;
        heartbeat.abort();

        let (state, value) = match verdict {
            Ok(Ok(v)) => ("resolved", v),
            Ok(Err(v)) => ("rejected", v),
            Err(halted) => {
                let kind = if halted.is_ok() { "task.halt" } else { "task.release" };
                let _ = self
                    .server
                    .process(&RequestEnvelope {
                        kind: kind.into(),
                        head: head(),
                        data: json!({ "id": self.task_id, "version": version }),
                    })
                    .await;
                return;
            }
        };
        let _ = self
            .server
            .process(&RequestEnvelope {
                kind: "task.fulfill".into(),
                head: head(),
                data: json!({
                    "id": self.task_id,
                    "version": version,
                    "action": {
                        "kind": "promise.settle",
                        "head": {},
                        "data": {
                            "id": self.task_id,
                            "state": state,
                            "value": {
                                "headers": { "content-type": "application/json" },
                                "data": b64_encode(&value)
                            }
                        }
                    }
                }),
            })
            .await;
    }
}

fn head() -> RequestHead {
    RequestHead {
        corr_id: format!("plugin-{}", fastrand::u64(..)),
        version: PROTOCOL_VERSION.to_string(),
        auth: None,
        debug_time: None,
    }
}

pub fn sanitize(promise_id: &str) -> String {
    let digest = fnv1a64_hex(promise_id.as_bytes());
    let safe: String = promise_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    format!("{safe}-{digest}")
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn b64_encode(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

pub(crate) fn b64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}
