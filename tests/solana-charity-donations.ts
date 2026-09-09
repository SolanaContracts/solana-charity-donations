import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { assert } from "chai";
import { SolanaCharityDonations } from "../target/types/solana_charity_donations";

describe("solana-charity-donations", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const program = anchor.workspace
    .solanaCharityDonations as Program<SolanaCharityDonations>;

  const authority = (provider.wallet as anchor.Wallet).payer;
  const donorA = Keypair.generate();
  const donorB = Keypair.generate();

  const campaignName = "flood-relief";

  const findCampaignPda = (name: string) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("campaign"), authority.publicKey.toBuffer(), Buffer.from(name)],
      program.programId
    )[0];

  const findDonationRecordPda = (campaign: PublicKey, donor: PublicKey) =>
    PublicKey.findProgramAddressSync(
      [Buffer.from("donation"), campaign.toBuffer(), donor.toBuffer()],
      program.programId
    )[0];

  const campaignPda = findCampaignPda(campaignName);

  before(async () => {
    for (const donor of [donorA, donorB]) {
      const sig = await provider.connection.requestAirdrop(
        donor.publicKey,
        2 * LAMPORTS_PER_SOL
      );
      await provider.connection.confirmTransaction(sig, "confirmed");
    }
  });

  it("initializes a campaign", async () => {
    await program.methods
      .initializeCampaign(
        campaignName,
        "Emergency flood relief fund",
        new BN(5 * LAMPORTS_PER_SOL),
        new BN(0)
      )
      .accounts({
        authority: authority.publicKey,
        campaign: campaignPda,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const campaign = await program.account.campaign.fetch(campaignPda);
    assert.equal(campaign.name, campaignName);
    assert.equal(campaign.amountRaised.toNumber(), 0);
    assert.equal(campaign.donorCount.toNumber(), 0);
  });

  it("accepts donations from multiple donors", async () => {
    await program.methods
      .donate(new BN(1 * LAMPORTS_PER_SOL))
      .accounts({
        donor: donorA.publicKey,
        campaign: campaignPda,
        donationRecord: findDonationRecordPda(campaignPda, donorA.publicKey),
        systemProgram: SystemProgram.programId,
      })
      .signers([donorA])
      .rpc();

    await program.methods
      .donate(new BN(0.5 * LAMPORTS_PER_SOL))
      .accounts({
        donor: donorB.publicKey,
        campaign: campaignPda,
        donationRecord: findDonationRecordPda(campaignPda, donorB.publicKey),
        systemProgram: SystemProgram.programId,
      })
      .signers([donorB])
      .rpc();

    const campaign = await program.account.campaign.fetch(campaignPda);
    assert.equal(campaign.amountRaised.toNumber(), 1.5 * LAMPORTS_PER_SOL);
    assert.equal(campaign.donorCount.toNumber(), 2);
  });

  it("lets the authority withdraw part of the raised funds", async () => {
    const balanceBefore = await provider.connection.getBalance(authority.publicKey);

    await program.methods
      .withdraw(new BN(0.5 * LAMPORTS_PER_SOL))
      .accounts({
        authority: authority.publicKey,
        campaign: campaignPda,
      })
      .rpc();

    const balanceAfter = await provider.connection.getBalance(authority.publicKey);
    assert.isAbove(balanceAfter, balanceBefore);

    const campaign = await program.account.campaign.fetch(campaignPda);
    assert.equal(campaign.amountWithdrawn.toNumber(), 0.5 * LAMPORTS_PER_SOL);
  });

  it("lets a donor refund their un-withdrawn donation", async () => {
    const donationRecordPda = findDonationRecordPda(campaignPda, donorB.publicKey);
    const balanceBefore = await provider.connection.getBalance(donorB.publicKey);

    await program.methods
      .requestRefund()
      .accounts({
        donor: donorB.publicKey,
        campaign: campaignPda,
        donationRecord: donationRecordPda,
      })
      .signers([donorB])
      .rpc();

    const balanceAfter = await provider.connection.getBalance(donorB.publicKey);
    assert.isAbove(balanceAfter, balanceBefore);

    const record = await program.account.donationRecord.fetch(donationRecordPda);
    assert.equal(record.amount.toNumber(), 0);

    const campaign = await program.account.campaign.fetch(campaignPda);
    assert.equal(campaign.amountRaised.toNumber(), 1 * LAMPORTS_PER_SOL);
  });

  it("rejects a refund once there is nothing left to refund", async () => {
    try {
      await program.methods
        .requestRefund()
        .accounts({
          donor: donorB.publicKey,
          campaign: campaignPda,
          donationRecord: findDonationRecordPda(campaignPda, donorB.publicKey),
        })
        .signers([donorB])
        .rpc();
      assert.fail("expected refund to fail");
    } catch (err) {
      assert.include(String(err), "InvalidAmount");
    }
  });

  it("prevents closing a campaign while donor A's funds are still pending", async () => {
    try {
      await program.methods
        .closeCampaign()
        .accounts({
          authority: authority.publicKey,
          campaign: campaignPda,
        })
        .rpc();
      assert.fail("expected close to fail");
    } catch (err) {
      assert.include(String(err), "PendingDonorFunds");
    }
  });

  it("allows close once all remaining funds are withdrawn", async () => {
    const campaign = await program.account.campaign.fetch(campaignPda);
    const remaining = campaign.amountRaised.sub(campaign.amountWithdrawn);

    await program.methods
      .withdraw(remaining)
      .accounts({
        authority: authority.publicKey,
        campaign: campaignPda,
      })
      .rpc();

    await program.methods
      .closeCampaign()
      .accounts({
        authority: authority.publicKey,
        campaign: campaignPda,
      })
      .rpc();

    const closed = await program.account.campaign.fetchNullable(campaignPda);
    assert.isNull(closed);
  });

  it("rejects donations after the deadline has passed", async () => {
    const expiredName = "expired-campaign";
    const expiredPda = findCampaignPda(expiredName);
    const nowSec = Math.floor(Date.now() / 1000);

    await program.methods
      .initializeCampaign(expiredName, "desc", new BN(1), new BN(nowSec + 2))
      .accounts({
        authority: authority.publicKey,
        campaign: expiredPda,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    await new Promise((resolve) => setTimeout(resolve, 3000));

    try {
      await program.methods
        .donate(new BN(1000))
        .accounts({
          donor: donorA.publicKey,
          campaign: expiredPda,
          donationRecord: findDonationRecordPda(expiredPda, donorA.publicKey),
          systemProgram: SystemProgram.programId,
        })
        .signers([donorA])
        .rpc();
      assert.fail("expected donate to fail");
    } catch (err) {
      assert.include(String(err), "DeadlinePassed");
    }
  });
});
