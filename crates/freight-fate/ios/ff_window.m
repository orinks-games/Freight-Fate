// The app's window: the scene it lives on, the safe area at the top of a
// phone's screen, and the system gestures kept off the game's own. Moved
// out of ff_touch.m unchanged.

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

// Ask iOS to defer its edge gestures (Home, Control Center, Notification
// Center) to a second swipe, so a stroke that starts at an edge still
// reaches the game. SDL's controller answers this from a hint; ours always
// answers every edge.
void ff_defer_screen_edges(UIViewController *controller) {
    if (!controller) {
        return;
    }
    SEL selector = @selector(preferredScreenEdgesDeferringSystemGestures);
    Method base = class_getInstanceMethod([UIViewController class], selector);
    IMP all_edges = imp_implementationWithBlock(^UIRectEdge(__unused id me) {
        return UIRectEdgeAll;
    });
    class_replaceMethod(object_getClass(controller), selector, all_edges, method_getTypeEncoding(base));
    [controller setNeedsUpdateOfScreenEdgesDeferringSystemGestures];
}

// Keep iOS's three-finger editing gestures off the root view controller too
// (see -[FFTouchView editingInteractionConfiguration]).
void ff_no_editing_gestures(UIViewController *controller) {
    if (!controller) {
        return;
    }
    if (@available(iOS 13.0, *)) {
        SEL selector = @selector(editingInteractionConfiguration);
        Method base = class_getInstanceMethod([UIResponder class], selector);
        IMP none = imp_implementationWithBlock(^UIEditingInteractionConfiguration(__unused id me) {
            return UIEditingInteractionConfigurationNone;
        });
        class_replaceMethod(object_getClass(controller), selector, none, method_getTypeEncoding(base));
    }
}

double ff_phone_safe_top_fraction(void *window) {
    UIWindow *ui_window = (__bridge UIWindow *)window;
    CGFloat height = CGRectGetHeight(ui_window.bounds);
    if (ui_window.traitCollection.userInterfaceIdiom != UIUserInterfaceIdiomPhone || height <= 0) {
        return 0;
    }
    return ui_window.safeAreaInsets.top / height;
}

// The scene lifecycle. iOS 27 refuses to launch an app built against its SDK
// without one (an Info.plist UIApplicationSceneManifest naming a scene
// delegate), and SDL2 predates scenes: it makes its windows with
// -initWithFrame:, which puts them on no scene, so they would never show.
// FFSceneDelegate is the manifest's delegate, and every window made with
// -initWithFrame: is moved onto the app's one window scene, whether it was
// made before the scene connected (SDL's launch window) or after (the game
// window). SDL still hears background and foreground through the
// UIApplication notifications, which iOS posts under scenes too, and so does
// ff_lifecycle.m.

static UIWindowScene *ff_scene;
static NSPointerArray *ff_unplaced_windows;

static void ff_place_window(UIWindow *window) {
    if (window.windowScene) {
        return;
    }
    if (ff_scene) {
        window.windowScene = ff_scene;
        return;
    }
    if (!ff_unplaced_windows) {
        ff_unplaced_windows = [NSPointerArray weakObjectsPointerArray];
    }
    [ff_unplaced_windows addPointer:(__bridge void *)window];
}

@interface FFSceneDelegate : UIResponder <UIWindowSceneDelegate>
@end

@implementation FFSceneDelegate
- (void)scene:(UIScene *)scene
    willConnectToSession:(__unused UISceneSession *)session
                 options:(__unused UISceneConnectionOptions *)options {
    if (![scene isKindOfClass:[UIWindowScene class]]) {
        return;
    }
    ff_scene = (UIWindowScene *)scene;
    // The window shown last before the scene existed is the one to show now.
    UIWindow *shown = nil;
    for (UIWindow *window in ff_unplaced_windows.allObjects) {
        window.windowScene = ff_scene;
        if (!window.hidden) {
            shown = window;
        }
    }
    ff_unplaced_windows = nil;
    [shown makeKeyAndVisible];
}

- (void)sceneDidDisconnect:(UIScene *)scene {
    if (scene == ff_scene) {
        ff_scene = nil;
    }
}
@end

@interface UIWindow (FFScene)
@end

@implementation UIWindow (FFScene)
+ (void)load {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        SEL original = @selector(initWithFrame:);
        SEL placed = @selector(initFFPlacedWithFrame:);
        Method placed_method = class_getInstanceMethod(self, placed);
        // UIWindow may inherit -initWithFrame: from UIView; give it its own
        // copy first so the exchange never touches every UIView.
        class_addMethod(self, original,
                        method_getImplementation(class_getInstanceMethod(self, original)),
                        method_getTypeEncoding(placed_method));
        method_exchangeImplementations(class_getInstanceMethod(self, original), placed_method);
    });
}

// An init-family name, so ARC gives it initializer ownership; after the
// exchange this calls the original -initWithFrame:.
- (instancetype)initFFPlacedWithFrame:(CGRect)frame {
    UIWindow *window = [self initFFPlacedWithFrame:frame];
    if (window) {
        ff_place_window(window);
    }
    return window;
}
@end
