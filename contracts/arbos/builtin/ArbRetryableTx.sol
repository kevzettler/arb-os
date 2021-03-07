
pragma solidity >=0.4.21 <0.7.0;

interface ArbRetryableTx {
    // Redeem a redeemable tx. This will revert if called by an L2 contract.
    // This reverts if txId does not exist.
    // This returns (success, retryable).
    //    (true, false) means that txId was redeemed, ran successfully, and can't be redeemed again.
    //    (false, true) means that txId was attempted but reverted for gas-related reasons, and is still available for redemption.
    //    (false, false) means that txId was attempted but reverted for other reasons, and can never be redeemed.
    //    This never returns (true, true).
    function redeem(uint txId) external returns(bool, bool);

    // Return the minimum lifetime of redeemable txs
    function getLifetime() external view returns(uint);

    // Return the timestamp when txId ages out, or zero if txId does not exist.
    // This could be in the past, because aged-out txs may not be discarded immediately.
    function getTimeout(uint txId) external view returns(uint);

    // Returns the cost, in L1 gas, of extending the lifetime of txId by an additional lifetime period.
    // Return value is (num, denom). If the L1 gas price is G, then cost if (G*num/denom).
    // Reverts if txId doesn't exist.
    function getKeepaliveCost(uint txId) external view returns(uint, uint);

    // Deposits callvalue into the sender's L2 account, then adds one lifetime period to the life of txId.
    // Reverts if txId does not exist or the sender has insufficient funds (after the deposit).
    // The cost will be (gasPrice * num / denom), where gasPrice is the gas price of the L1 tx that submits the
    //            tx in which this is run, and (num, denom) is what getKeepaliveCost returns for txId.
    function keepalive(uint txId) external payable;

    event LifetimeExtended(uint txId, uint newTimeout);
    event RedemptionResult(uint txId, bool success, bool retryable);
}

