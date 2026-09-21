import Foundation
import CoreGraphics
import UniformTypeIdentifiers
import Darwin

/// Thin Swift wrapper around `dt_extract` / `dt_free_bitmap`.
enum DitherThumb {
  enum Kind {
    case project
    case pattern

    var dt: DtKind {
      switch self {
      case .project: return DT_KIND_PROJECT
      case .pattern: return DT_KIND_PATTERN
      }
    }

    static func from(url: URL) -> Kind? {
      if let type = try? url.resourceValues(forKeys: [.contentTypeKey]).contentType {
        if type.conforms(to: UTType(exportedAs: "com.dither.app.project")) {
          return .project
        }
        if type.conforms(to: UTType(exportedAs: "com.dither.app.pattern")) {
          return .pattern
        }
      }
      switch url.pathExtension.lowercased() {
      case "dyproj": return .project
      case "dyuki": return .pattern
      default: return nil
      }
    }
  }

  static let expectedAbi: UInt32 = 1

  /// Decode preview via Rust. Returns a premultiplied-last CGImage, or nil.
  static func extract(url: URL, kind: Kind, maxSide: UInt32) -> CGImage? {
    guard dt_abi_version() == expectedAbi else { return nil }
    guard let fh = try? FileHandle(forReadingFrom: url) else { return nil }
    defer { try? fh.close() }

    let size: UInt64
    do {
      size = try fh.seekToEnd()
      try fh.seek(toOffset: 0)
    } catch {
      return nil
    }
    if size == 0 { return nil }

    let ctx = FileReadCtx(handle: fh)
    var io = DtIo(
      ctx: Unmanaged.passUnretained(ctx).toOpaque(),
      size: size,
      read_at: ditherThumbReadAt
    )

    var bmp = DtBitmap(width: 0, height: 0, rgba: nil, len: 0)
    let status = dt_extract(&io, kind.dt, maxSide, 1, &bmp)
    defer { dt_free_bitmap(&bmp) }

    guard status == DT_OK, let ptr = bmp.rgba, bmp.len > 0, bmp.width > 0, bmp.height > 0 else {
      return nil
    }
    let data = Data(bytes: ptr, count: Int(bmp.len))
    return makeCGImage(rgba: data, width: Int(bmp.width), height: Int(bmp.height))
  }

  private static func makeCGImage(rgba: Data, width: Int, height: Int) -> CGImage? {
    let bytesPerRow = width * 4
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
    guard let provider = CGDataProvider(data: rgba as CFData) else { return nil }
    return CGImage(
      width: width,
      height: height,
      bitsPerComponent: 8,
      bitsPerPixel: 32,
      bytesPerRow: bytesPerRow,
      space: colorSpace,
      bitmapInfo: bitmapInfo,
      provider: provider,
      decode: nil,
      shouldInterpolate: true,
      intent: .defaultIntent
    )
  }
}

final class FileReadCtx {
  let handle: FileHandle
  init(handle: FileHandle) { self.handle = handle }
}

/// C-compatible `pread` bridge (no captures — required for `DtIo.read_at`).
private func ditherThumbReadAt(
  _ ctxPtr: UnsafeMutableRawPointer?,
  _ offset: UInt64,
  _ buf: UnsafeMutablePointer<UInt8>?,
  _ len: Int,
  _ outRead: UnsafeMutablePointer<Int>?
) -> Int32 {
  guard let ctxPtr, let buf, let outRead else { return 1 }
  let ctx = Unmanaged<FileReadCtx>.fromOpaque(ctxPtr).takeUnretainedValue()
  let fd = ctx.handle.fileDescriptor
  var total = 0
  while total < len {
    let n = pread(fd, buf.advanced(by: total), len - total, off_t(offset) + off_t(total))
    if n < 0 {
      if errno == EINTR { continue }
      return 1
    }
    if n == 0 { break }
    total += Int(n)
  }
  outRead.pointee = total
  return 0
}

func fit(_ imageSize: CGSize, into maxSize: CGSize) -> CGSize {
  let sx = maxSize.width / max(imageSize.width, 1)
  let sy = maxSize.height / max(imageSize.height, 1)
  let s = min(sx, sy)
  return CGSize(width: imageSize.width * s, height: imageSize.height * s)
}
