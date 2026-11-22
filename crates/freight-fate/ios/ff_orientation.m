#import <UIKit/UIKit.h>

// Tilt steering holds the screen's orientation while the player is at the
// wheel. SDL's view controller answers supportedInterfaceOrientations from
// the SDL_IOS_ORIENTATIONS hint each time UIKit asks; the game changes the
// hint, and this asks UIKit to ask again. From iOS 16 that is the view
// controller's setNeedsUpdateOfSupportedInterfaceOrientations; before it,
// UIKit asks on the next rotation, and attemptRotationToDeviceOrientation
// asks now.
void ff_refresh_orientations(void *window) {
    UIWindow *ui_window = (__bridge UIWindow *)window;
    UIViewController *controller = ui_window.rootViewController;
    if (!controller) {
        return;
    }
    if (@available(iOS 16.0, *)) {
        [controller setNeedsUpdateOfSupportedInterfaceOrientations];
    } else {
        [UIViewController attemptRotationToDeviceOrientation];
    }
}
