/* openKWL runs behind the stock Helios bootloader (0x08000000–0x08007FFF).
 *
 * Bootloader contract (observed behaviour of the stock bootloader BASBL110):
 * - It boots the image only if the initial stack pointer lies in 0x20010000–0x2001FFFF, so the
 *   stack must start below the end of RAM (cortex-m-rt's default 0x20020000 is refused).
 * - Image area 0x08008000–0x0803FDFF (0x37E00 bytes), CRC trailer at 0x0803FE00
 *   (tools/openkwl-pack). Sector 1 (0x08004000–0x08007FFF) holds the stock settings; keep it.
 * - It does not set VTOR (cortex-m-rt's `set-vtor` does) and leaves the clocks on HSI.
 *
 * Chip: STM32F207VCT6 (100 pins, 256 KB flash). RAM: 112 KB SRAM1 + 16 KB SRAM2, contiguous. */
MEMORY
{
  FLASH : ORIGIN = 0x08008000, LENGTH = 0x37E00
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}

/* Not at the very top: the bootloader's jump sets MSP to this value and then still reads up
 * to 16 bytes above it. With 0x2001FFF8 that read went past the end of RAM and faulted inside the
 * bootloader, before our first instruction (dark board, no restart). The stock image uses
 * 0x2001ABA8. */
_stack_start = ORIGIN(RAM) + LENGTH(RAM) - 256;
