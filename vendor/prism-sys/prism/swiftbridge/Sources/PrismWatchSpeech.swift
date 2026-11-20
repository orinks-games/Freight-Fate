// SPDX-License-Identifier: MPL-2.0

import Accessibility
import Foundation
import SwiftUI

@objc(PrismWatchSpeechSink)
public final class PrismWatchSpeechSink: NSObject {
  @objc public static var isSupported: Bool {
    if #available(watchOS 10.0, *) {
      return true
    }
    return false
  }

  @objc public func announce(_ text: String, interrupt: Bool) -> Bool {
    guard #available(watchOS 10.0, *) else {
      return false
    }
    var attributed = AttributedString(text)
    attributed.accessibilitySpeechAnnouncementPriority = interrupt ? .high : .low
    let announcement = attributed
    DispatchQueue.main.async {
      AccessibilityNotification.Announcement(announcement).post()
    }
    return true
  }
}
