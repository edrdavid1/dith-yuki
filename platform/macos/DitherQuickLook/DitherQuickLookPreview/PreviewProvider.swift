import QuickLook
import QuickLookUI
import UniformTypeIdentifiers
import Cocoa

@objc(PreviewProvider)
final class PreviewProvider: QLPreviewProvider, QLPreviewingController {
  func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
    guard let kind = DitherThumb.Kind.from(url: request.fileURL),
          let image = DitherThumb.extract(url: request.fileURL, kind: kind, maxSide: 1024)
    else {
      throw CocoaError(.fileReadCorruptFile)
    }
    let pixel = CGSize(width: CGFloat(image.width), height: CGFloat(image.height))
    let size = fit(pixel, into: CGSize(width: 1024, height: 1024))
    return QLPreviewReply(contextSize: size, isBitmap: true) { context, _ in
      context.interpolationQuality = .high
      context.draw(image, in: CGRect(origin: .zero, size: size))
      return .init()
    }
  }
}
