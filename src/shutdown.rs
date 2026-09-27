use std::io;

pub struct Shutdown {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl Shutdown {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    pub async fn wait(mut self) {
        #[cfg(unix)]
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if result.is_err() {
                    tracing::error!("failed to listen for interrupt signal");
                }
            }
            _ = self.terminate.recv() => {}
        }
        #[cfg(not(unix))]
        if tokio::signal::ctrl_c().await.is_err() {
            tracing::error!("failed to listen for interrupt signal");
        }
        tracing::info!("shutdown requested");
    }
}
