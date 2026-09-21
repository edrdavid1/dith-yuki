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
    let pixel = CGSize(width: CGFloat(image.width), height: CGFloat(image.height))
    let size = fit(pixel, into: request.maximumSize)
    handler(
      QLThumbnailReply(contextSize: size) { context in
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(origin: .zero, size: size))
        return true
      },
      nil
    )
  }
}
