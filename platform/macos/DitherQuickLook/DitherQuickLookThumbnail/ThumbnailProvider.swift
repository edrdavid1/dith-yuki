import QuickLookThumbnailing
import UniformTypeIdentifiers
import Cocoa

final class ThumbnailProvider: QLThumbnailProvider {
  override func provideThumbnail(
    for request: QLFileThumbnailRequest,
    _ handler: @escaping (QLThumbnailReply?, (any Error)?) -> Void
  ) {
    guard let kind = DitherThumb.Kind.from(url: request.fileURL) else {
      handler(nil, CocoaError(.fileReadCorruptFile))
      return
    }
    let maxPx = UInt32(
      min(1024, ceil(max(request.maximumSize.width, request.maximumSize.height) * request.scale))
    )
    guard let image = DitherThumb.extract(url: request.fileURL, kind: kind, maxSide: maxPx) else {
      handler(nil, CocoaError(.fileReadCorruptFile))
      return
    }
    let size = pointSizeFromBitmap(image, scale: request.scale)
    handler(
      QLThumbnailReply(contextSize: size) { context in
        context.interpolationQuality = .high
        drawFullContext(image, in: context, contextSize: size)
        return true
      },
      nil
    )
  }
}
