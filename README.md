# Mir HSA IR

This project is a not yet working (WIP).

## Goals


The rest of this readme is just my notes about impl details.

## Impl Notes

SPIRV doesn't support function pointers. Thus anytime a closure is used,
the SPIRV module that receives the closure is recompiled, ie Function 
pointers are a pi type.

