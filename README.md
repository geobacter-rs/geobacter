# Mir HSA IR

This project is a not yet working (WIP). You'll need to use 
[this Rust fork](https://github.com/DiamondLovesYou/rust) 
to use this project (consult Rustup on how to use).

## Goals

* Single source accelerator programming.

The rest of these are accomplished via runtime libraries. Thus 
these can be dropped if desired, but they are a part of my desired 
target "market".

* Long running programs 
  * Ie spending 30 minutes optimizating isn't out of the question 
  in terms of practicality,
* Distributed memory architectures.
  * Obviously, a very hard problem, but we don't require a perfect 
  solution upfront: start running the program and dedicate a core 
  to finding a more optimal solution, then recodegen.
* Use input data to inform optimizations.

The rest of this readme is just my notes about impl details.

## Impl Notes

SPIRV doesn't support function pointers. Thus anytime a closure is used,
the SPIRV module that receives the closure is recompiled, ie Function 
pointers are a pi type. As far as compile time is concerned, 
this project doesn't need to restrict the functions it can accept. 
The acceptability of a function can be left to the specific target 
platform codegen crate. 

## Misc Notes

Your Rust toolchain build *must* have LLVM assertions off.
