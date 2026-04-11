use anchor_lang::prelude::*;

declare_id!("H7jDuDVA6y2RXvpnofKSwLJ9E8Ubht8WCttDj1LDP7h7");

#[program]
pub mod deposit_contract {
    use super::*;

    pub fn deposit(ctx: Context<Deposit>, amount: u64, zcash_address: String) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;
        let clock = Clock::get()?;

        deposit.user = ctx.accounts.user.key();
        deposit.amount = amount;
        deposit.zcash_address = zcash_address;
        deposit.timestamp = clock.unix_timestamp;
        deposit.refund_delay = 3600;
        deposit.is_processed = false;
        deposit.is_refunded = false;
        deposit.zcash_tx_id = "".to_string();

        Ok(())
    }

    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;
        let clock = Clock::get()?;

        require!(!deposit.is_processed, DepositError::AlreadyProcessed);
        require!(!deposit.is_refunded, DepositError::AlreadyRefunded);
        require!(
            clock.unix_timestamp > deposit.timestamp + deposit.refund_delay,
            DepositError::RefundNotReady
        );

        deposit.is_refunded = true;
        Ok(())
    }

    pub fn confirm_shielding(ctx: Context<ConfirmShielding>, zcash_tx_id: String) -> Result<()> {
        let deposit = &mut ctx.accounts.deposit_account;

        require!(!deposit.is_processed, DepositError::AlreadyProcessed);
        require!(!deposit.is_refunded, DepositError::AlreadyRefunded);

        deposit.is_processed = true;
        deposit.zcash_tx_id = zcash_tx_id;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        space = 8 + 32 + 8 + 256 + 8 + 8 + 1 + 1 + 256,
        seeds = [b"deposit", user.key().as_ref()],
        bump
    )]
    pub deposit_account: Account<'info, DepositAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        seeds = [b"deposit", deposit_account.user.as_ref()],
        bump,
        constraint = deposit_account.user == user.key()
    )]
    pub deposit_account: Account<'info, DepositAccount>,
}

#[derive(Accounts)]
pub struct ConfirmShielding<'info> {
    #[account(mut)]
    pub relayer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"deposit", deposit_account.user.as_ref()],
        bump
    )]
    pub deposit_account: Account<'info, DepositAccount>,
}

#[account]
pub struct DepositAccount {
    pub user: Pubkey,
    pub amount: u64,
    pub zcash_address: String,
    pub timestamp: i64,
    pub refund_delay: i64,
    pub is_processed: bool,
    pub is_refunded: bool,
    pub zcash_tx_id: String,
}

#[error_code]
pub enum DepositError {
    AlreadyProcessed,
    AlreadyRefunded,
    RefundNotReady,
}