import QuickLook
import QuickLookUI
import UniformTypeIdentifiers
import Cocoa

// No `@objc(PreviewProvider)`: that registers the class as `PreviewProvider`,
// but Info.plist asks for `$(PRODUCT_MODULE_NAME).PreviewProvider`. PlugInKit
// then fails to instantiate the extension and Quick Look falls back to the type icon.
// ThumbnailProvider has no custom ObjC name and loads; this class must match it.
final class PreviewProvider: QLPreviewProvider, QLPreviewingController {
  func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
    guard let kind = DitherThumb.Kind.from(url: request.fileURL),
          let image = DitherThumb.extract(url: request.fileURL, kind: kind, maxSide: 1024)
    else {
      throw CocoaError(.fileReadCorruptFile)
    }
    let size = pointSizeFromBitmap(image, scale: 1)
    // Xcode 16+/SDK 26: drawUsing closure returns Void (not Bool / DrawingResult).
    return QLPreviewReply(contextSize: size, isBitmap: true, drawUsing: { context, _ in
      context.interpolationQuality = .high
      drawFullContext(image, in: context, contextSize: size)
    })
  }
}
