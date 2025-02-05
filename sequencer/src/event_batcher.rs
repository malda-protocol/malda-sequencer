use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, error, info};

use crate::event_processor::ProcessedEvent;
use sequencer::logger::PipelineLogger;
pub struct EventBatcher {
    input_rx: Receiver<ProcessedEvent>,
    output_tx: Sender<Vec<ProcessedEvent>>,
    batch_size: usize,
    timeout: Duration,
    logger: PipelineLogger,
    current_batch: Vec<ProcessedEvent>,
    last_event_time: Instant,
}

impl EventBatcher {
    pub fn new(
        input_rx: Receiver<ProcessedEvent>,
        output_tx: Sender<Vec<ProcessedEvent>>,
        batch_size: usize,
        timeout_ms: u64,
        logger: PipelineLogger,
    ) -> Self {
        Self {
            input_rx,
            output_tx,
            batch_size,
            timeout: Duration::from_millis(timeout_ms),
            logger,
            current_batch: Vec::with_capacity(batch_size),
            last_event_time: Instant::now(),
        }
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!(parent: &self.logger, "Starting event batcher");

        loop {
            tokio::select! {
                // Check for new events
                event = self.input_rx.recv() => {
                    match event {
                        Some(event) => {
                            self.current_batch.push(event);
                            self.last_event_time = Instant::now();

                            if self.current_batch.len() >= self.batch_size {
                                self.send_batch().await?;
                            }
                        }
                        None => {
                            debug!(parent: &self.logger, "Input channel closed");
                            // Send any remaining events before shutting down
                            if !self.current_batch.is_empty() {
                                self.send_batch().await?;
                            }
                            break;
                        }
                    }
                }

                // Check timeout
                _ = sleep(self.timeout) => {
                    if !self.current_batch.is_empty() 
                        && self.last_event_time.elapsed() >= self.timeout 
                    {
                        self.send_batch().await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_batch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.current_batch.is_empty() {
            return Ok(());
        }

        let batch = std::mem::replace(&mut self.current_batch, Vec::with_capacity(self.batch_size));
        debug!(
            parent: &self.logger,
            "Sending batch of {} events", batch.len()
        );

        if let Err(e) = self.output_tx.send(batch).await {
            error!(
                parent: &self.logger,
                "Failed to send batch: {}", e
            );
            return Err(Box::new(e));
        }

        Ok(())
    }
}
