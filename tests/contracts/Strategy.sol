// SPDX-License-Identifier: AGPL-3.0
// Feel free to change the license, but this is what we use

// Feel free to change this version of Solidity. We support >=0.6.0 <0.7.0;
pragma solidity 0.6.12;
pragma experimental ABIEncoderV2;

import {
    BaseStrategy
} from "@yearnvaults/contracts/BaseStrategy.sol";
import "@openzeppelin/contracts/math/Math.sol";
import {
    SafeERC20,
    SafeMath,
    IERC20,
    Address
} from "@openzeppelin/contracts/token/ERC20/SafeERC20.sol";

import "./ySwap/ITradeFactory.sol";

// Import interfaces for many popular DeFi projects, or add your own!

import "../interfaces/tokemak/ILiquidityEthPool.sol";
import "../interfaces/tokemak/IRewards.sol";

contract Strategy is BaseStrategy {
    using SafeERC20 for IERC20;
    using Address for address;
    using SafeMath for uint256;

    ILiquidityEthPool internal constant tokemakEthPool =
        ILiquidityEthPool(0xD3D13a578a53685B4ac36A1Bab31912D2B2A2F36);

    IManager internal constant tokemakManager =
        IManager(0xA86e412109f77c45a3BC1c5870b880492Fb86A14);

    // From Tokemak docs: tABC tokens represent your underlying claim to the assets
    // you deposited into the token reactor, available to be redeemed 1:1 at any time
    IERC20 internal constant tWETH =
        IERC20(0xD3D13a578a53685B4ac36A1Bab31912D2B2A2F36);

    IRewards internal constant tokemakRewards =
        IRewards(0x79dD22579112d8a5F7347c5ED7E609e60da713C5);

    IERC20 internal constant tokeToken =
        IERC20(0x2e9d63788249371f1DFC918a52f8d799F4a38C94);

    bool internal isOriginal = true;

    address public tradeFactory = address(0);

    constructor(address _vault) BaseStrategy(_vault) public {
        // You can set these parameters on deployment to whatever you want
        // maxReportDelay = 6300;
        // profitFactor = 100;
        // debtThreshold = 0;
    }

     // this will only be called by the clone function
    function initialize(
        address _vault,
        address _strategist
    ) external {
         _initialize(_vault, _strategist, _strategist, _strategist);
    }

    event Cloned(address indexed clone);
    function cloneTokemakWeth(
        address _vault,
        address _strategist
    ) external returns (address payable newStrategy) {
        require(isOriginal);

        bytes20 addressBytes = bytes20(address(this));

        assembly {
            // EIP-1167 bytecode
            let clone_code := mload(0x40)
            mstore(clone_code, 0x3d602d80600a3d3981f3363d3d373d3d3d363d73000000000000000000000000)
            mstore(add(clone_code, 0x14), addressBytes)
            mstore(add(clone_code, 0x28), 0x5af43d82803e903d91602b57fd5bf30000000000000000000000000000000000)
            newStrategy := create(0, clone_code, 0x37)
        }

        Strategy(newStrategy).initialize(_vault, _strategist);

        emit Cloned(newStrategy);
    }

    function name() external view override returns (string memory) {
        return "StrategyTokemakWETH";
    }

    function estimatedTotalAssets() public view override returns (uint256) {
        // 1 tWETH = 1 WETH *guaranteed*
        return twethBalance().add(wantBalance());
    }

    function prepareReturn(uint256 _debtOutstanding)
        internal
        override
        returns (
            uint256 _profit,
            uint256 _loss,
            uint256 _debtPayment
        )
    {
        require(tradeFactory != address(0), "Trade factory must be set.");
        // How much do we owe to the vault?
        uint256 totalDebt = vault.strategies(address(this)).totalDebt;

        uint256 totalAssets = estimatedTotalAssets();

        if (totalAssets >= totalDebt) {
            _profit = totalAssets.sub(totalDebt);
        } else {
            _loss = totalDebt.sub(totalAssets);
        }

        (uint256 _liquidatedAmount, ) = liquidatePosition(_debtOutstanding);

        _debtPayment = Math.min(_debtOutstanding, _liquidatedAmount);
    }

    function adjustPosition(uint256 _debtOutstanding) internal override {
        uint256 wantBalance = wantBalance();

        if (wantBalance > _debtOutstanding) {
            uint256 _amountToInvest = wantBalance.sub(_debtOutstanding);

            _checkAllowance(address(tokemakEthPool), address(want), _amountToInvest);

            try tokemakEthPool.deposit(_amountToInvest) {} catch {}
        }
    }

    function liquidatePosition(uint256 _amountNeeded)
        internal
        override
        returns (uint256 _liquidatedAmount, uint256 _loss)
    {
        // NOTE: Maintain invariant `_liquidatedAmount + _loss <= _amountNeeded`

        uint256 _existingLiquidAssets = wantBalance();

        if (_existingLiquidAssets >= _amountNeeded) {
            return (_amountNeeded, 0);
        }

        uint256 _amountToWithdraw = _amountNeeded.sub(_existingLiquidAssets);

        (uint256 _cycleIndexWhenWithdrawable, uint256 _requestedWithdrawAmount) =
            tokemakEthPool.requestedWithdrawals(address(this));

        if (_requestedWithdrawAmount == 0 || _cycleIndexWhenWithdrawable > tokemakManager.getCurrentCycleIndex()) {
            tokemakEthPool.requestWithdrawal(_amountToWithdraw);

            return (_existingLiquidAssets, 0);
        }

        // Cannot withdraw more than withdrawable
        _amountToWithdraw = Math.min(
            _amountToWithdraw,
            _requestedWithdrawAmount
        );

        try tokemakEthPool.withdraw(_amountToWithdraw, false) {
            uint256 _newLiquidAssets = wantBalance();

            _liquidatedAmount = Math.min(_newLiquidAssets, _amountNeeded);

            if (_liquidatedAmount < _amountNeeded) {
                // If we couldn't liquidate the full amount needed, start the withdrawal process for the remaining
                tokemakEthPool.requestWithdrawal(_amountNeeded.sub(_liquidatedAmount));
            }
        } catch {
            return (_existingLiquidAssets, 0);
        }
    }

    function liquidateAllPositions()
        internal
        override
        returns (uint256 _amountFreed)
    {
        (_amountFreed, ) = liquidatePosition(estimatedTotalAssets());
    }

    function prepareMigration(address _newStrategy) internal override {
        uint256 _amountToTransfer = twethBalance();

        tWETH.safeTransfer(_newStrategy, _amountToTransfer);
    }

    // Override this to add all tokens/tokenized positions this contract manages
    // on a *persistent* basis (e.g. not just for swapping back to want ephemerally)
    // NOTE: Do *not* include `want`, already included in `sweep` below
    //
    // Example:
    //
    //    function protectedTokens() internal override view returns (address[] memory) {
    //      address[] memory protected = new address[](3);
    //      protected[0] = tokenA;
    //      protected[1] = tokenB;
    //      protected[2] = tokenC;
    //      return protected;
    //    }
    function protectedTokens()
        internal
        view
        override
        returns (address[] memory)
    {}

    function ethToWant(uint256 _amtInWei)
        public
        view
        virtual
        override
        returns (uint256)
    {
        return _amtInWei;
    }

    // ----------------- YSWAPS FUNCTIONS ---------------------

    function setTradeFactory(address _tradeFactory) external onlyGovernance {
        if (tradeFactory != address(0)) {
            _removeTradeFactoryPermissions();
        }

        // approve and set up trade factory
        tokeToken.safeApprove(_tradeFactory, type(uint256).max);
        ITradeFactory tf = ITradeFactory(_tradeFactory);
        tf.enable(address(tokeToken), address(want));
        tradeFactory = _tradeFactory;
    }

    function removeTradeFactoryPermissions() external onlyEmergencyAuthorized {
        _removeTradeFactoryPermissions();

    }
    function _removeTradeFactoryPermissions() internal {
        tokeToken.safeApprove(tradeFactory, 0);
        tradeFactory = address(0);
    }

    // ----------------- STRATEGIST-MANAGED FUNCTIONS ---------

    function requestWithdrawal(uint256 amount)
        external
        onlyEmergencyAuthorized
    {
        tokemakEthPool.requestWithdrawal(amount);
    }

    function claimRewards(
        IRewards.Recipient calldata _recipient,
        uint8 _v,
        bytes32 _r,
        bytes32 _s // bytes calldata signature
    )
        external
        onlyVaultManagers
    {
        require(_recipient.wallet == address(this), "Recipient wallet must be strategy");
        tokemakRewards.claim(_recipient, _v, _r, _s);
    }

    // ----------------- SUPPORT FUNCTIONS ----------

    function tokeTokenBalance()
        public
        view
        returns (uint256)
    {
        return tokeToken.balanceOf(address(this));
    }

    function wantBalance()
        public
        view
        returns (uint256)
    {
        return want.balanceOf(address(this));
    }

    function twethBalance()
        public
        view
        returns (uint256)
    {
        return tWETH.balanceOf(address(this));
    }

    function _checkAllowance(
        address _contract,
        address _token,
        uint256 _amount
    ) internal {
        if (IERC20(_token).allowance(address(this), _contract) < _amount) {
            IERC20(_token).safeApprove(_contract, 0);
            IERC20(_token).safeApprove(_contract, _amount);
        }
    }

}
