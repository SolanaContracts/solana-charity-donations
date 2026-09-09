use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("CRFmqtbgKoKERdFzFuBTRS5YuDfLYphfRGmGo6Zcn3Qa");

const MAX_NAME_LEN: usize = 50;
const MAX_DESCRIPTION_LEN: usize = 200;

#[program]
pub mod solana_charity_donations {
    use super::*;

    pub fn initialize_campaign(
        ctx: Context<InitializeCampaign>,
        name: String,
        description: String,
        goal_lamports: u64,
        deadline_unix: i64,
    ) -> Result<()> {
        require!(name.len() <= MAX_NAME_LEN, CharityError::NameTooLong);
        require!(
            description.len() <= MAX_DESCRIPTION_LEN,
            CharityError::DescriptionTooLong
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            deadline_unix == 0 || deadline_unix > now,
            CharityError::InvalidDeadline
        );

        let campaign = &mut ctx.accounts.campaign;
        campaign.authority = ctx.accounts.authority.key();
        campaign.name = name;
        campaign.description = description;
        campaign.goal_lamports = goal_lamports;
        campaign.deadline_unix = deadline_unix;
        campaign.amount_raised = 0;
        campaign.amount_withdrawn = 0;
        campaign.donor_count = 0;
        campaign.bump = ctx.bumps.campaign;

        Ok(())
    }

    pub fn donate(ctx: Context<Donate>, amount: u64) -> Result<()> {
        require!(amount > 0, CharityError::InvalidAmount);

        let now = Clock::get()?.unix_timestamp;
        let deadline = ctx.accounts.campaign.deadline_unix;
        require!(
            deadline == 0 || now <= deadline,
            CharityError::DeadlinePassed
        );

        transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.donor.to_account_info(),
                    to: ctx.accounts.campaign.to_account_info(),
                },
            ),
            amount,
        )?;

        let record = &mut ctx.accounts.donation_record;
        if record.amount == 0 && record.donor == Pubkey::default() {
            record.donor = ctx.accounts.donor.key();
            record.campaign = ctx.accounts.campaign.key();
            record.bump = ctx.bumps.donation_record;
            ctx.accounts.campaign.donor_count = ctx
                .accounts
                .campaign
                .donor_count
                .checked_add(1)
                .ok_or(CharityError::MathOverflow)?;
        }
        record.amount = record
            .amount
            .checked_add(amount)
            .ok_or(CharityError::MathOverflow)?;

        let campaign = &mut ctx.accounts.campaign;
        campaign.amount_raised = campaign
            .amount_raised
            .checked_add(amount)
            .ok_or(CharityError::MathOverflow)?;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, CharityError::InvalidAmount);

        let campaign_info = ctx.accounts.campaign.to_account_info();
        let rent_exempt_minimum = Rent::get()?.minimum_balance(campaign_info.data_len());
        let withdrawable = campaign_info
            .lamports()
            .saturating_sub(rent_exempt_minimum);
        require!(amount <= withdrawable, CharityError::InsufficientFunds);

        **campaign_info.try_borrow_mut_lamports()? -= amount;
        **ctx
            .accounts
            .authority
            .to_account_info()
            .try_borrow_mut_lamports()? += amount;

        let campaign = &mut ctx.accounts.campaign;
        campaign.amount_withdrawn = campaign
            .amount_withdrawn
            .checked_add(amount)
            .ok_or(CharityError::MathOverflow)?;

        Ok(())
    }

    pub fn request_refund(ctx: Context<RequestRefund>) -> Result<()> {
        let refund_amount = ctx.accounts.donation_record.amount;
        require!(refund_amount > 0, CharityError::InvalidAmount);

        let campaign_info = ctx.accounts.campaign.to_account_info();
        let rent_exempt_minimum = Rent::get()?.minimum_balance(campaign_info.data_len());
        let available = campaign_info
            .lamports()
            .saturating_sub(rent_exempt_minimum);
        require!(refund_amount <= available, CharityError::InsufficientFunds);

        **campaign_info.try_borrow_mut_lamports()? -= refund_amount;
        **ctx
            .accounts
            .donor
            .to_account_info()
            .try_borrow_mut_lamports()? += refund_amount;

        ctx.accounts.donation_record.amount = 0;

        let campaign = &mut ctx.accounts.campaign;
        campaign.amount_raised = campaign
            .amount_raised
            .checked_sub(refund_amount)
            .ok_or(CharityError::MathOverflow)?;

        Ok(())
    }

    pub fn close_campaign(ctx: Context<CloseCampaign>) -> Result<()> {
        let campaign = &ctx.accounts.campaign;
        require!(
            campaign.amount_raised == campaign.amount_withdrawn,
            CharityError::PendingDonorFunds
        );
        Ok(())
    }
}

#[account]
pub struct Campaign {
    pub authority: Pubkey,
    pub name: String,
    pub description: String,
    pub goal_lamports: u64,
    pub deadline_unix: i64,
    pub amount_raised: u64,
    pub amount_withdrawn: u64,
    pub donor_count: u64,
    pub bump: u8,
}

impl Campaign {
    pub const MAX_SIZE: usize = 8 // discriminator
        + 32 // authority
        + 4 + MAX_NAME_LEN // name
        + 4 + MAX_DESCRIPTION_LEN // description
        + 8 // goal_lamports
        + 8 // deadline_unix
        + 8 // amount_raised
        + 8 // amount_withdrawn
        + 8 // donor_count
        + 1; // bump
}

#[account]
pub struct DonationRecord {
    pub donor: Pubkey,
    pub campaign: Pubkey,
    pub amount: u64,
    pub bump: u8,
}

impl DonationRecord {
    pub const MAX_SIZE: usize = 8 // discriminator
        + 32 // donor
        + 32 // campaign
        + 8 // amount
        + 1; // bump
}

#[derive(Accounts)]
#[instruction(name: String)]
pub struct InitializeCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = Campaign::MAX_SIZE,
        seeds = [b"campaign", authority.key().as_ref(), name.as_bytes()],
        bump,
    )]
    pub campaign: Account<'info, Campaign>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Donate<'info> {
    #[account(mut)]
    pub donor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"campaign", campaign.authority.as_ref(), campaign.name.as_bytes()],
        bump = campaign.bump,
    )]
    pub campaign: Account<'info, Campaign>,

    #[account(
        init_if_needed,
        payer = donor,
        space = DonationRecord::MAX_SIZE,
        seeds = [b"donation", campaign.key().as_ref(), donor.key().as_ref()],
        bump,
    )]
    pub donation_record: Account<'info, DonationRecord>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ CharityError::Unauthorized,
        seeds = [b"campaign", campaign.authority.as_ref(), campaign.name.as_bytes()],
        bump = campaign.bump,
    )]
    pub campaign: Account<'info, Campaign>,
}

#[derive(Accounts)]
pub struct RequestRefund<'info> {
    #[account(mut)]
    pub donor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"campaign", campaign.authority.as_ref(), campaign.name.as_bytes()],
        bump = campaign.bump,
    )]
    pub campaign: Account<'info, Campaign>,

    #[account(
        mut,
        has_one = donor @ CharityError::Unauthorized,
        seeds = [b"donation", campaign.key().as_ref(), donor.key().as_ref()],
        bump = donation_record.bump,
    )]
    pub donation_record: Account<'info, DonationRecord>,
}

#[derive(Accounts)]
pub struct CloseCampaign<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority @ CharityError::Unauthorized,
        seeds = [b"campaign", campaign.authority.as_ref(), campaign.name.as_bytes()],
        bump = campaign.bump,
        close = authority,
    )]
    pub campaign: Account<'info, Campaign>,
}

#[error_code]
pub enum CharityError {
    #[msg("Campaign name must be 50 characters or fewer")]
    NameTooLong,
    #[msg("Campaign description must be 200 characters or fewer")]
    DescriptionTooLong,
    #[msg("Deadline must be zero (no deadline) or in the future")]
    InvalidDeadline,
    #[msg("This campaign's deadline has passed")]
    DeadlinePassed,
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Campaign does not have enough withdrawable funds for this operation")]
    InsufficientFunds,
    #[msg("Campaign still has donor funds that have not been withdrawn or refunded")]
    PendingDonorFunds,
    #[msg("Signer is not authorized to perform this action")]
    Unauthorized,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
