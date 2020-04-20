A small example writing a relatively fast GPU GEMM kernel with Geobacter.

This example demonstrates usage of LDS and specialization params.

You'll need to compile with `RUSTFLAGS=-Z always-encode-mir -Z always-emit-metadata`. 
This used to be set automatically, but not anymore.

## Performance

On the author's Radeon VII, 2.7Tflops is achieved. An RX 580 gets just over 1.0Tflops.
