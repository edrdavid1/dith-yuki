/* Generated / hand-maintained C header for dither-thumb FFI (ABI v1).
 * Keep in sync with crates/dither-thumb-ffi and dt_abi_version().
 */
#ifndef DITHER_THUMB_H
#define DITHER_THUMB_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
  DT_OK = 0,
  DT_NOT_AVAILABLE = 1,
  DT_UNSUPPORTED = 2,
  DT_LIMIT = 3,
  DT_CORRUPT = 4,
  DT_TIMEOUT = 5,
  DT_INTERNAL = 6,
  DT_BAD_ARG = 7
} DtStatus;

typedef enum {
  DT_KIND_PROJECT = 1,
  DT_KIND_PATTERN = 2
} DtKind;

typedef struct {
  void *ctx;
  uint64_t size;
  int (*read_at)(void *ctx, uint64_t offset, uint8_t *buf, size_t len, size_t *out_read);
} DtIo;

typedef struct {
  uint32_t width;
  uint32_t height;
  uint8_t *rgba;
  size_t len;
} DtBitmap;

uint32_t dt_abi_version(void);
DtStatus dt_extract(const DtIo *io, DtKind kind, uint32_t max_side,
                    int premultiply, DtBitmap *out);
void dt_free_bitmap(DtBitmap *bmp);

#ifdef __cplusplus
}
#endif

#endif /* DITHER_THUMB_H */
