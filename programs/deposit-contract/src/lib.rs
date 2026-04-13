use anchor_lang::prelude::*;

declare_id!("62WLfheJDNnbnmUtd8PUHhQMBYBQyWTXc3S8LBktfUa9");

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum DepositStatus {
    Pending,
    Processing,
    Completed,
    Refunded,
}

#[account]
pub struct DepositAccount {
    pub user: Pubkey,
    pub amount: u64,
    pub zcash_address: String,
    pub timestamp: i64,
    pub refund_delay: i64,
    pub status: DepositStatus,
    pub zcash_tx_id: String,
    pub processed_by: Pubkey,
    pub deposit_id: u64,
}

impl DepositAccount {
    pub const SPACE: usize = 8 + 32 + 8 + 200 + 8 + 8 + 1 + 200 + 32 + 8;
}

#[account]
pub struct AuthorityConfig {
    pub relayer_key: Pubkey,
    pub is_paused: bool,
    pub escrow_bump: u8,
    pub deposit_counter: u64,
}

impl AuthorityConfig {
    pub const SPACE: usize = 8 + 32 + 1 + 1 + 8;
}

#[program]
pub mod deposit_contract {
    use super::*;

    pub fn initialize_authority(ctx: Context<InitializeAuthority>, relayer_key: Pubkey) -> Result<()> {
        let authority = &mut ctx.accounts.authority;
        authority.relayer_key = relayer_key;
        authority.is_paused = false;
        authority.deposit_counter = 0;
        
        let (_, bump) = Pubkey::find_program_address(&[b"escrow"], ctx.program_id);
        authority.escrow_bump = bump;
        
        Ok(())
    }

    pub fn update_relayer(ctx: Context<UpdateRelayer>, new_relayer_key: Pubkey) -> Result<()> {
        ctx.accounts.authority.relayer_key = new_relayer_key;
        Ok(())
    }

    pub fn pause(ctx: Context<Pause>, is_paused: bool) -> Result<()> {
        ctx.accounts.authority.is_paused = is_paused;
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64, zcash_address: String) -> Result<()> {
        require!(!ctx.accounts.authority.is_paused, DepositError::Paused);
        
        let deposit = &mut ctx.accounts.deposit_account;
        let clock = Clock::get()?;
        let timestamp = clock.unix_timestamp;
        let deposit_id = ctx.accounts.authority.deposit_counter;

        require!(amount > 0, DepositError::InvalidAmount);
        require!(zcash_address.len() <= 200, DepositError::AddressTooLong);
        require!(amount <= ctx.accounts.user.lamports(), DepositError::InsufficientFunds);

        deposit.user = ctx.accounts.user.key();
        deposit.amount = amount;
        deposit.zcash_address = zcash_address;
        deposit.timestamp = timestamp;
        deposit.refund_delay = 3600;
        deposit.status = DepositStatus::Pending;
        deposit.zcash_tx_id = "".to_string();
        deposit.processed_by = Pubkey::default();
        deposit.deposit_id = deposit_id;

        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.user.to_account_info(),
                    to: ctx.accounts.escrow.to_account_info(),
                },
            ),
            amount,
        )?;

        ctx.accounts.authority.deposit_counter += 1;

        emit!(DepositEvent {
            user: deposit.user,
            amount: deposit.amount,
            zcash_address: deposit.zcash_address.clone(),
            timestamp: deposit.timestamp,
            deposit_id: deposit.deposit_id,
        });

        Ok(())
    }

    pub fn start_processing(ctx: Context<StartProcessing>) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;

        require!(!ctx.accounts.authority.is_paused, DepositError::Paused);
        require!(
            deposit.status == DepositStatus::Pending,
            DepositError::InvalidStatus
        );
        require!(
            ctx.accounts.relayer.key() == ctx.accounts.authority.relayer_key,
            DepositError::Unauthorized
        );

        deposit.status = DepositStatus::Processing;
        deposit.processed_by = ctx.accounts.relayer.key();

        emit!(ProcessingEvent {
            user: deposit.user,
            deposit_id: deposit.deposit_id,
        });

        Ok(())
    }

    pub fn confirm_shielding(ctx: Context<ConfirmShielding>, zcash_tx_id: String) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;

        require!(!ctx.accounts.authority.is_paused, DepositError::Paused);
        require!(
            deposit.status == DepositStatus::Processing,
            DepositError::InvalidStatus
        );
        require!(
            ctx.accounts.relayer.key() == ctx.accounts.authority.relayer_key,
            DepositError::Unauthorized
        );
        require!(zcash_tx_id.len() <= 200, DepositError::AddressTooLong);

        deposit.status = DepositStatus::Completed;
        deposit.zcash_tx_id = zcash_tx_id.clone();

        emit!(ConfirmEvent {
            user: deposit.user,
            deposit_id: deposit.deposit_id,
            zcash_tx_id,
        });

        Ok(())
    }

    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;
        let clock = Clock::get()?;

        require!(
            deposit.status == DepositStatus::Pending || deposit.status == DepositStatus::Processing,
            DepositError::InvalidStatus
        );

        let refund_available_time = deposit.timestamp.checked_add(deposit.refund_delay)
            .ok_or(DepositError::Overflow)?;
        
        require!(
            clock.unix_timestamp > refund_available_time,
            DepositError::RefundNotReady
        );

        let bump = ctx.accounts.authority.escrow_bump;
        let seeds = &[&b"escrow"[..], &[bump]];
        
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.escrow.to_account_info(),
                    to: ctx.accounts.user.to_account_info(),
                },
                &[seeds],
            ),
            deposit.amount,
        )?;

        deposit.status = DepositStatus::Refunded;

        emit!(RefundEvent {
            user: deposit.user,
            amount: deposit.amount,
            deposit_id: deposit.deposit_id,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeAuthority<'info> {
    #[account(init, payer = admin, space = AuthorityConfig::SPACE, seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateRelayer<'info> {
    #[account(mut, seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
    #[account(constraint = admin.key() == authority.relayer_key @ DepositError::Unauthorized)]
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct Pause<'info> {
    #[account(mut, seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
    #[account(constraint = admin.key() == authority.relayer_key @ DepositError::Unauthorized)]
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        space = DepositAccount::SPACE,
        seeds = [b"deposit", user.key().as_ref(), &authority.deposit_counter.to_le_bytes()],
        bump
    )]
    pub deposit_account: Account<'info, DepositAccount>,
    /// CHECK: This is the escrow PDA that holds SOL deposits. Security is enforced by:
    /// - PDA seeds validation ([b"escrow"])
    /// - Bump stored in authority.escrow_bump
    /// - Only this program can sign transfers from this account
    #[account(
        mut,
        seeds = [b"escrow"],
        bump = authority.escrow_bump
    )]
    pub escrow: UncheckedAccount<'info>,
    #[account(mut, seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StartProcessing<'info> {
    #[account(mut)]
    pub deposit_account: Account<'info, DepositAccount>,
    pub relayer: Signer<'info>,
    #[account(seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
}

#[derive(Accounts)]
pub struct ConfirmShielding<'info> {
    #[account(mut)]
    pub deposit_account: Account<'info, DepositAccount>,
    pub relayer: Signer<'info>,
    #[account(seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
}

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        seeds = [b"deposit", user.key().as_ref(), &deposit_account.deposit_id.to_le_bytes()],
        bump,
        constraint = deposit_account.user == user.key()
    )]
    pub deposit_account: Account<'info, DepositAccount>,
    /// CHECK: This is the escrow PDA that holds SOL deposits. Security is enforced by:
    /// - PDA seeds validation ([b"escrow"])
    /// - Bump stored in authority.escrow_bump
    /// - Only this program can sign transfers from this account
    #[account(
        mut,
        seeds = [b"escrow"],
        bump = authority.escrow_bump
    )]
    pub escrow: UncheckedAccount<'info>,
    #[account(seeds = [b"authority"], bump)]
    pub authority: Account<'info, AuthorityConfig>,
    pub system_program: Program<'info, System>,
}

#[event]
pub struct DepositEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub zcash_address: String,
    pub timestamp: i64,
    pub deposit_id: u64,
}

#[event]
pub struct ProcessingEvent {
    pub user: Pubkey,
    pub deposit_id: u64,
}

#[event]
pub struct ConfirmEvent {
    pub user: Pubkey,
    pub deposit_id: u64,
    pub zcash_tx_id: String,
}

#[event]
pub struct RefundEvent {
    pub user: Pubkey,
    pub amount: u64,
    pub deposit_id: u64,
}

#[error_code]
pub enum DepositError {
    #[msg("Refund not available yet")]
    RefundNotReady,
    #[msg("Invalid deposit status for this operation")]
    InvalidStatus,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Zcash address too long")]
    AddressTooLong,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Protocol is paused")]
    Paused,
    #[msg("Insufficient funds")]
    InsufficientFunds,
    #[msg("Arithmetic overflow")]
    Overflow,
}
