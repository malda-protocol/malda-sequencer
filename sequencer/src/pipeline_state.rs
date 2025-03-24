use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error};

#[derive(Debug)]
pub enum PipelineStatus {
    Received,
    Processed,
    ProofGenerated,
    TransactionSubmitted,
    Completed,
    Failed { error: String },
}

pub struct DatabaseBackedPipeline {
    // Fast in-memory channels
    event_tx: mpsc::Sender<RawEvent>,
    proof_tx: mpsc::Sender<ProcessedEvent>,
    tx_tx: mpsc::Sender<ProofReadyEvent>,
    
    // Persistent state
    db: Database,
}

impl DatabaseBackedPipeline {
    pub async fn process_event(&self, event: RawEvent) -> Result<()> {
        // Record receipt of event
        self.db.update_status(&event.tx_hash, PipelineStatus::Received).await?;
        
        // Process in memory
        let processed = match process_event(event) {
            Ok(p) => p,
            Err(e) => {
                // Record failure and return
                self.db.update_status(
                    &event.tx_hash, 
                    PipelineStatus::Failed { error: e.to_string() }
                ).await?;
                return Err(e);
            }
        };
        
        // Record successful processing
        self.db.record_processed_event(&processed).await?;
        self.db.update_status(&event.tx_hash, PipelineStatus::Processed).await?;
        
        // Continue pipeline via channel
        if let Err(e) = self.proof_tx.send(processed.clone()).await {
            error!("Failed to send to proof generator: {}", e);
            // Can recover later by querying DB for Processed events
        }

        Ok(())
    }
    
    pub async fn generate_proof(&self, event: ProcessedEvent) -> Result<()> {
        let proof = match generate_proof(event) {
            Ok(p) => p,
            Err(e) => {
                self.db.update_status(
                    &event.tx_hash,
                    PipelineStatus::Failed { error: e.to_string() }
                ).await?;
                return Err(e);
            }
        };
        
        // Record proof generation
        self.db.record_proof(&proof).await?;
        self.db.update_status(&event.tx_hash, PipelineStatus::ProofGenerated).await?;
        
        // Continue pipeline
        if let Err(e) = self.tx_tx.send(proof.clone()).await {
            error!("Failed to send to transaction manager: {}", e);
            // Can recover by querying DB for ProofGenerated events
        }

        Ok(())
    }

    pub async fn recover_failed_events(&self) -> Result<()> {
        // Find events that failed or got stuck
        let failed_events = self.db.get_events_by_status(PipelineStatus::Failed).await?;
        let stuck_processed = self.db.get_events_by_status(PipelineStatus::Processed).await?;
        
        info!(
            "Recovering {} failed events and {} stuck processed events",
            failed_events.len(),
            stuck_processed.len()
        );

        // Reprocess failed events
        for event in failed_events {
            self.process_event(event).await?;
        }

        // Regenerate proofs for stuck processed events
        for event in stuck_processed {
            self.generate_proof(event).await?;
        }

        Ok(())
    }
} 