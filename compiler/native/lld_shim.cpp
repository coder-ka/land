#include <cstddef>

#include "lld/Common/Driver.h"
#include "llvm/ADT/ArrayRef.h"
#include "llvm/Config/llvm-config.h"
#include "llvm/Support/raw_ostream.h"

#if LLVM_VERSION_MAJOR >= 15
#ifdef _WIN32
LLD_HAS_DRIVER(coff)

// The prebuilt COFF driver references every compiled LLVM target. Land's
// Windows compiler deliberately ships only the X86 backend.
#define LAND_UNUSED_TARGET(name)                                               \
  extern "C" void LLVMInitialize##name##TargetInfo() {}                        \
  extern "C" void LLVMInitialize##name##Target() {}                            \
  extern "C" void LLVMInitialize##name##TargetMC() {}                          \
  extern "C" void LLVMInitialize##name##AsmParser() {}                         \
  extern "C" void LLVMInitialize##name##AsmPrinter() {}

LAND_UNUSED_TARGET(AArch64)
LAND_UNUSED_TARGET(ARM)
LAND_UNUSED_TARGET(BPF)
LAND_UNUSED_TARGET(WebAssembly)
LAND_UNUSED_TARGET(RISCV)

extern "C" void LLVMInitializeNVPTXTargetInfo() {}
extern "C" void LLVMInitializeNVPTXTarget() {}
extern "C" void LLVMInitializeNVPTXTargetMC() {}
extern "C" void LLVMInitializeNVPTXAsmPrinter() {}
#else
LLD_HAS_DRIVER(elf)
#endif
#endif

extern "C" bool land_lld_link(int argc, const char **argv) {
#ifdef _WIN32
  return lld::coff::link(llvm::ArrayRef<const char *>(argv, argc), llvm::outs(),
                         llvm::errs(), false, false);
#else
  return lld::elf::link(llvm::ArrayRef<const char *>(argv, argc), llvm::outs(),
                        llvm::errs(), false, false);
#endif
}
