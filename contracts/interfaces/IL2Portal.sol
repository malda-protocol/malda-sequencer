// SPDX-License-Identifier: UNDEFINED

pragma solidity ^0.8.20;

interface IL2Portal {
    function supply(uint256 _amount) external returns (bool);

    function borrow(uint256 _amount) external returns (bool);

    function repay(uint256 _amount) external returns (bool);

    function withdraw(uint256 _amount) external returns (bool);
}
