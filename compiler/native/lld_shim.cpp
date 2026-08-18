#include <cstddef>

#include "lld/Common/Driver.h"
#include "llvm/ADT/ArrayRef.h"
#include "llvm/Support/raw_ostream.h"

extern "C" bool land_lld_link(int argc, const char **argv) {
  return lld::elf::link(llvm::ArrayRef<const char *>(argv, argc), llvm::outs(),
                        llvm::errs(), false, false);
}
