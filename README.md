# :construction: The Geobacter Rust Compiler and Runtime :construction:

## What is Geobacter?

Geobacter is a framework to support single source accelerator programming,
without requiring compiling from source twice. However, Geobacter is *not* a
JIT; the expectation is that kernels are invoked repeatedly and are possibly
also expensive to run. In fact, due to the nature of single function kernels,
Geobacter enables a number of LLVM options which would too costly to run for
all crates.

The name Geobacter comes from the first discovered bacteria which can oxidize
organic components using iron oxide as an electron acceptor.

Currently, AMDGPU and HPC is the focus.

## How does this work?

In the Rust type system, every `fn` is given a unique type. Thus, we can refer
to a function definition just by its type. In our Rust fork, we use special
driver-defined intrinsic functions to essentially return a form of the Rust
compiler's `ty::Instance<'_>`, along with other info specific to the desired
target (for example, to support SPIR-V pipeline descriptions)

At runtime, we respool the compiler driver (into the runtime driver) using
metadata from every dependency. We then lookup the real `DefId` and, through
some compiler provider API tricks, get the LLVM codegen crate to generate IR for
the corresponding function and all of its dependencies. To be clear though, this
is not a JIT, and is ill-suited to JIT-esk things; for instance, we enable extra
LLVM optimizations, like Polly as well as increase the optimization search space.
Thus, while the optimizations are *relatively* fast, for the purposes of a JIT,
they are too slow.

There is an in process cache, in the form of compiler generated statics, so
kernels aren't codegenned more than once per accelerator target.

## Status (ie what does and doesn't work)

Generally, everything up to LLVM optimizations works for all targets. 
Getting an id for a specific function at compile time works. At runtime, loading
every crate's metadata and using that to set up a pseudo Rust driver works, as well 
taking the aforementioned function id and running codegen for it including
optimizations and (if applicable) sending it to a target machine a second time works.

### AMDGPU

This target works out of the box, though dispatch requires some unsafety as there are 
still unsolved foot guns surrounding passing references to device inaccessible memory 
(like the stack!) to kernels. Additionally, kernel outputs have to be explicitly 
passed by pointer, because `&mut ` must be unique, but all workitems share the same 
argument values!

"Extra" features:
* Nice interface to specify kernel launch bounds and get the workitem/workgroup ids 
  efficiently,
* Device visible host memory allocators,
* Device memory allocation, but this can't be used in `Box`/etc because large BAR 
  can't be guaranteed,
* Device textures,
* Device side signals,
* (Mostly) Safe LDS (workgroup memory) interfaces for a few usage scenarios.

TODOs :construction:
* Nicer cross-bar interfaces.
* Adapt OpenCL std functions to have Geobacter equivalents.
* Safe output writes: two workitems must not create mutable references to the 
  same variable.
* Device side enqueue: need mechanisms to embed child kernel image handles in the parent 
  kernel.
* Device -> host MPSC channels.

### Vulkan/SPIRV

Vulkan/SPIR-V isn't nearly as well supported as AMDGPU, but "simple" compute kernels 
should work, and there currently isn't a guide for its use.

Geobacter requires that your Vulkan implementation support the physical storage buffer 
addresses and variable pointers extensions.

<!--
Geobacter supports (or will, once a proc-macro gets written help with this)
single definition descriptor set/binding numbers; that is, both the resulting
SPIR-V global and the host code will use the same numbers when they refer to the
global or binding type, respectively.

The hope is to also allow creating entire graphics pipelines; ie vertex,
geometry, tess (eval and control), raytracing, and fragment "kernels", which are
then codgenned into a single SPIR-V module.
-->

### Cuda

No support.

## How to get the toolchain?

ATM, we don't have prebuilt compilers for you to download, let alone the ability to download 
directly from `rustup`.

So you'll need to build the Rust toolchain yourself. See BUILD.md.

TODO :construction: offer prebuilt packages.

## How do I code with this?

See CODING.md! :^)

## Who is working on this?

Richard Diamond works on this in his free time.

New contributors are absolutely welcome.
