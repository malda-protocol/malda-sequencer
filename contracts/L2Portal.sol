// SPDX-License-Identifier: UNDEFINED

pragma solidity ^0.8.20;

import {Ownable} from "lib/openzeppelin-contracts/contracts/access/Ownable.sol";
import {IERC20} from "lib/openzeppelin-contracts/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "lib/openzeppelin-contracts/contracts/token/ERC20/utils/SafeERC20.sol";

import {IL2Portal} from "./interfaces/IL2Portal.sol";

contract L2Portal is Ownable, IL2Portal {
    using SafeERC20 for IERC20;

    event Supply(address user, uint256 amount);

    IERC20 public constant UNDERLYING_TOKEN = IERC20(address(0));

    uint256 public totalBalance;

    constructor(address _owner) Ownable(_owner) {}

    function supply(uint256 _amount) external override returns (bool) {
        UNDERLYING_TOKEN.transferFrom(msg.sender, address(this), _amount);
        totalBalance += _amount;
        emit Supply(msg.sender, _amount);
        return true;
    }

    // TODO
    function borrow(uint256 _amount) external override returns (bool) {
        return true;
    }

    // TODO
    function repay(uint256 _amount) external override returns (bool) {
        return true;
    }

    // TODO
    function withdraw(uint256 _amount) external override returns (bool) {
        return true;
    }
}
