//! End-to-end: spawn Anvil (Prague + EIP-7825 tx gas cap), store a random 330 KB proof with
//! every strategy, read it back, and compare byte for byte.

use alloy::{
    node_bindings::Anvil,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use proof_store::{contracts::Deployment, election_id, load_proof, strategies::Ctx, Strategy};

#[tokio::test(flavor = "multi_thread")]
async fn all_strategies_roundtrip_330kb() -> eyre::Result<()> {
    let anvil = Anvil::new().args(["--hardfork", "prague", "--enable-tx-gas-limit"]).try_spawn()?;
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let provider = ProviderBuilder::new().wallet(signer).connect_http(anvil.endpoint_url()).erased();
    let deployment = Deployment::deploy(&provider).await?;
    let ctx = Ctx { provider: provider.clone(), deployment };

    let proof = load_proof("random:330000")?;
    let file = std::env::temp_dir().join(format!("proof-store-test-{}.bin", std::process::id()));
    std::fs::write(&file, &proof)?;
    let uri = format!("file://{}", file.display());

    for s in Strategy::ALL {
        let id = election_id(&format!("test-election-{s}"));
        let r = s.store(&ctx, id, &proof, Some(&uri)).await?;
        assert!(r.gas_used > 0, "{s}: no gas recorded");
        let back = s.retrieve(&ctx, id).await?;
        assert_eq!(back, proof, "{s}: roundtrip mismatch");
        // write-once registry: a second store of the same election must fail
        if s == Strategy::Commitment {
            assert!(s.store(&ctx, id, &proof, Some(&uri)).await.is_err());
        }
        println!("{s:<11} txs={:<3} gas={:>11} blob_gas={}", r.tx_hashes.len(), r.gas_used, r.blob_gas_used);
    }
    let _ = std::fs::remove_file(file);
    assert!(provider.get_block_number().await? > 0);
    Ok(())
}
