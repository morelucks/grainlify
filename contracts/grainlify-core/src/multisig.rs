//! Multisig approval engine used by Grainlify upgrade flows.
//!
//! Proposal identifiers are allocated from a monotonic counter and are treated
//! as stable handles for subsequent approval and execution steps.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Vec};

#[contracttype]
enum DataKey {
    Config,
    Proposal(u64),
    ProposalCounter,
    Paused,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiSigConfig {
    pub signers: Vec<Address>,
    pub threshold: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct Proposal {
    pub approvals: Vec<Address>,
    pub executed: bool,
    pub expiry: u64,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum MultiSigError {
    NotSigner,
    AlreadyApproved,
    ProposalNotFound,
    ProposalAlreadyExists,
    AlreadyExecuted,
    ThresholdNotMet,
    InvalidThreshold,
    ContractPaused,
    StateInconsistent,
    ProposalExpired,
    ProposalCancelled,
}

pub struct MultiSig;

impl MultiSig {
    pub fn init(env: &Env, signers: Vec<Address>, threshold: u32) {
        if threshold == 0 || threshold > signers.len() {
            panic!("{:?}", MultiSigError::InvalidThreshold);
        }

        let config = MultiSigConfig { signers, threshold };
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u64);
    }

    pub fn propose(env: &Env, proposer: Address, expiry: u64) -> u64 {
        proposer.require_auth();

        let config = Self::get_config(env);
        Self::assert_signer(&config, &proposer);

        let mut counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0);

        counter += 1;

        let proposal = Proposal {
            approvals: Vec::new(env),
            executed: false,
            expiry,
            cancelled: false,
        };

        if env.storage().instance().has(&DataKey::Proposal(counter)) {
            panic!("{:?}", MultiSigError::ProposalAlreadyExists);
        }

        env.storage().instance().set(&DataKey::Proposal(counter), &proposal);
        env.storage().instance().set(&DataKey::ProposalCounter, &counter);

        env.events().publish((symbol_short!("proposal"),), counter);

        counter
    }

    pub fn approve(env: &Env, proposal_id: u64, signer: Address) {
        signer.require_auth();

        let config = Self::get_config(env);
        Self::assert_signer(&config, &signer);

        let mut proposal = Self::get_proposal(env, proposal_id);

        if proposal.executed {
            panic!("{:?}", MultiSigError::AlreadyExecuted);
        }

        if proposal.cancelled {
            panic!("{:?}", MultiSigError::ProposalCancelled);
        }

        if Self::proposal_is_expired(env, &proposal) {
            panic!("{:?}", MultiSigError::ProposalExpired);
        }

        if proposal.approvals.contains(&signer) {
            panic!("{:?}", MultiSigError::AlreadyApproved);
        }

        proposal.approvals.push_back(signer.clone());

        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events()
            .publish((symbol_short!("approved"),), (proposal_id, signer));
    }

    pub fn can_execute(env: &Env, proposal_id: u64) -> bool {
        if Self::is_contract_paused(env) || Self::is_state_inconsistent(env) {
            return false;
        }

        let config = Self::get_config(env);
        let proposal = Self::get_proposal(env, proposal_id);

        if proposal.executed || proposal.cancelled {
            return false;
        }

        if Self::proposal_is_expired(env, &proposal) {
            return false;
        }

        proposal.approvals.len() >= config.threshold
    }

    pub fn is_expired(env: &Env, proposal_id: u64) -> bool {
        let proposal = Self::get_proposal(env, proposal_id);
        Self::proposal_is_expired(env, &proposal)
    }

    pub fn is_cancelled(env: &Env, proposal_id: u64) -> bool {
        let proposal = Self::get_proposal(env, proposal_id);
        proposal.cancelled
    }

    pub fn cancel(env: &Env, proposal_id: u64, signer: Address) {
        signer.require_auth();

        let config = Self::get_config(env);
        Self::assert_signer(&config, &signer);

        let mut proposal = Self::get_proposal(env, proposal_id);

        if proposal.executed {
            panic!("{:?}", MultiSigError::AlreadyExecuted);
        }
        if proposal.cancelled {
            panic!("{:?}", MultiSigError::ProposalCancelled);
        }

        proposal.cancelled = true;

        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events()
            .publish((symbol_short!("cancelled"),), (proposal_id, signer));
    }

    pub fn mark_executed(env: &Env, proposal_id: u64) {
        let mut proposal = Self::get_proposal(env, proposal_id);

        if proposal.executed {
            panic!("{:?}", MultiSigError::AlreadyExecuted);
        }

        if !Self::can_execute(env, proposal_id) {
            panic!("{:?}", MultiSigError::ThresholdNotMet);
        }

        proposal.executed = true;

        env.storage()
            .instance()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events()
            .publish((symbol_short!("executed"),), proposal_id);
    }

    pub fn pause(env: &Env, signer: Address) {
        signer.require_auth();

        let config = Self::get_config(env);
        Self::assert_signer(&config, &signer);

        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((symbol_short!("paused"),), signer);
    }

    pub fn unpause(env: &Env, signer: Address) {
        signer.require_auth();

        let config = Self::get_config(env);
        Self::assert_signer(&config, &signer);

        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((symbol_short!("unpause"),), signer);
    }

    pub fn is_contract_paused(env: &Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    pub fn is_state_inconsistent(env: &Env) -> bool {
        match Self::get_config_opt(env) {
            Some(config) => config.threshold == 0 || config.threshold > config.signers.len() as u32,
            None => false,
        }
    }

    pub fn get_config_opt(env: &Env) -> Option<MultiSigConfig> {
        env.storage().instance().get(&DataKey::Config)
    }

    pub fn get_proposal_opt(env: &Env, proposal_id: u64) -> Option<Proposal> {
        env.storage().instance().get(&DataKey::Proposal(proposal_id))
    }

    pub fn set_config(env: &Env, config: MultiSigConfig) {
        if config.threshold == 0 || config.threshold > config.signers.len() as u32 {
            panic!("{:?}", MultiSigError::InvalidThreshold);
        }
        env.storage().instance().set(&DataKey::Config, &config);
    }

    pub fn clear_config(env: &Env) {
        env.storage().instance().remove(&DataKey::Config);
    }

    fn get_config(env: &Env) -> MultiSigConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("multisig not initialized")
    }

    fn get_proposal(env: &Env, proposal_id: u64) -> Proposal {
        env.storage()
            .instance()
            .get(&DataKey::Proposal(proposal_id))
            .unwrap_or_else(|| panic!("{:?}", MultiSigError::ProposalNotFound))
    }

    fn assert_signer(config: &MultiSigConfig, signer: &Address) {
        if !config.signers.contains(signer) {
            panic!("{:?}", MultiSigError::NotSigner);
        }
    }

    fn proposal_is_expired(env: &Env, proposal: &Proposal) -> bool {
        proposal.expiry != 0 && env.ledger().timestamp() >= proposal.expiry
    }
}
