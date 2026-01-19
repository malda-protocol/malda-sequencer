//! SignerService and actor pattern implementation.

use alloy_consensus::SignableTransaction;
use alloy_network::TxSigner;
use alloy_primitives::{Address, B256};
use alloy_signer::{Signature, Signer};
use tokio::sync::{mpsc, oneshot};

use crate::error::SignerError;

/// Messages sent to the signer actor.
pub(crate) enum SignerMessage {
    SignHash {
        hash: B256,
        respond_to: oneshot::Sender<Result<Signature, SignerError>>,
    },
}

/// A cloneable handle to communicate with a signer actor.
///
/// This handle is agnostic to the underlying signer implementation.
/// Clone this handle to share signing capabilities across tasks.
///
/// Implements [`Signer`] and [`TxSigner`] traits for use with [`EthereumWallet`].
///
/// # Example
///
/// ```ignore
/// use malda_signer::SignerService;
/// use alloy::network::EthereumWallet;
///
/// let service = SignerService::new(signer);
/// let wallet = EthereumWallet::from(service);
/// ```
#[derive(Clone, Debug)]
pub struct SignerService {
    sender: mpsc::Sender<SignerMessage>,
    /// Cached address for fast synchronous access.
    address: Address,
    /// Cached chain_id for fast synchronous access.
    chain_id: Option<u64>,
}

impl SignerService {
    /// Create a new signer service from any type implementing [`Signer`].
    ///
    /// This spawns a background actor task that processes signing requests.
    /// The address and chain_id are cached for synchronous access.
    pub fn new<S>(signer: S) -> Self
    where
        S: Signer + Send + Sync + 'static,
    {
        // Cache these before moving signer into actor.
        let address = signer.address();
        let chain_id = signer.chain_id();

        let (sender, receiver) = mpsc::channel(32);
        tokio::spawn(run_signer_actor(receiver, signer));

        Self {
            sender,
            address,
            chain_id,
        }
    }

    /// Sign a hash asynchronously.
    ///
    /// # Errors
    ///
    /// Returns [`SignerError::ActorDropped`] if the actor was dropped,
    /// or [`SignerError::Signing`] if the signing operation failed.
    pub async fn sign_hash(&self, hash: B256) -> Result<Signature, SignerError> {
        let (send, recv) = oneshot::channel();
        let msg = SignerMessage::SignHash {
            hash,
            respond_to: send,
        };
        self.sender
            .send(msg)
            .await
            .map_err(|_| SignerError::ActorDropped)?;
        recv.await.map_err(|_| SignerError::ActorDropped)?
    }

    /// Get the signer's Ethereum address.
    ///
    /// This is a synchronous operation as the address is cached.
    pub fn address(&self) -> Address {
        self.address
    }

    /// Get the signer's chain ID.
    ///
    /// This is a synchronous operation as the chain ID is cached.
    pub fn chain_id(&self) -> Option<u64> {
        self.chain_id
    }

    /// Verify the signer is working by signing a test hash.
    ///
    /// Call this at startup to fail fast if credentials or permissions are invalid.
    /// For AWS KMS, this verifies the `kms:Sign` permission works.
    ///
    /// # Errors
    ///
    /// Returns an error if signing fails (invalid credentials, missing permissions, etc.)
    pub async fn health_check(&self) -> Result<(), SignerError> {
        self.sign_hash(B256::ZERO).await?;
        Ok(())
    }
}

// Implement Signer trait for EthereumWallet compatibility.
#[async_trait::async_trait]
impl Signer for SignerService {
    async fn sign_hash(&self, hash: &B256) -> Result<Signature, alloy_signer::Error> {
        self.sign_hash(*hash)
            .await
            .map_err(|e| alloy_signer::Error::Other(Box::new(e)))
    }

    fn address(&self) -> Address {
        self.address
    }

    fn chain_id(&self) -> Option<u64> {
        self.chain_id
    }

    fn set_chain_id(&mut self, _chain_id: Option<u64>) {
        // CAUTION: This is a no-op. Chain ID is fixed at construction time.
        // The underlying signer's chain_id is immutable.
    }
}

// Implement TxSigner trait for EthereumWallet compatibility.
#[async_trait::async_trait]
impl TxSigner<Signature> for SignerService {
    fn address(&self) -> Address {
        self.address
    }

    async fn sign_transaction(
        &self,
        tx: &mut dyn SignableTransaction<Signature>,
    ) -> alloy_signer::Result<Signature> {
        let hash = tx.signature_hash();
        self.sign_hash(hash)
            .await
            .map_err(|e| alloy_signer::Error::Other(Box::new(e)))
    }
}

async fn run_signer_actor<S>(mut receiver: mpsc::Receiver<SignerMessage>, signer: S)
where
    S: Signer + Send + Sync,
{
    while let Some(msg) = receiver.recv().await {
        match msg {
            SignerMessage::SignHash { hash, respond_to } => {
                let result = signer.sign_hash(&hash).await.map_err(SignerError::from);
                let _ = respond_to.send(result);
            }
        }
    }
}
