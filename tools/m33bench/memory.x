/* QEMU mps2-an505: secure aliases of SSRAM1 (code) and SSRAM2/3 (data). */
MEMORY {
  FLASH : ORIGIN = 0x10000000, LENGTH = 4096K
  RAM   : ORIGIN = 0x38000000, LENGTH = 2048K
}
