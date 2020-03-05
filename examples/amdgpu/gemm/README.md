A small example writing a *fast* GPU GEMM kernel with Geobacter.

This example demonstrates usage of LDS and specialization params.

## Running

You'll have to set `GEOBACTER_USE_LLC=1`. Without, you'll hit an assertion in the AMDGPU target machine. It is
 unknown why LLC succeeds here.

## Performance

On the author's Radeon VII, 1.57Tflops is achieved. An RX 580 gets just under 617Gflops.
