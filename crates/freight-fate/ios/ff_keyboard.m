#import <GameController/GameController.h>
#import <UIKit/UIKit.h>
#import <dispatch/dispatch.h>
#import <objc/runtime.h>

// SDL's UIKit key commands synthesize another arrow press independently of
// its hardware keyboard path (GCKeyboard or the view's UIPress callbacks).
void ff_use_hardware_vertical_arrows(void *window) {
    UIWindow *ui_window = (__bridge UIWindow *)window;
    UIViewController *controller = ui_window.rootViewController;
    if (!controller) {
        return;
    }
    Class cls = object_getClass(controller);
    SEL selector = @selector(keyCommands);
    Method method = class_getInstanceMethod(cls, selector);
    if (!method) {
        return;
    }
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSArray<UIKeyCommand *> *(*original)(id, SEL) =
            (NSArray<UIKeyCommand *> *(*)(id, SEL))method_getImplementation(method);
        IMP filtered = imp_implementationWithBlock(^NSArray<UIKeyCommand *> *(id receiver) {
            NSArray<UIKeyCommand *> *commands = original(receiver, selector);
            NSMutableArray<UIKeyCommand *> *kept = [NSMutableArray arrayWithCapacity:commands.count];
            for (UIKeyCommand *command in commands) {
                BOOL vertical = [command.input isEqualToString:UIKeyInputUpArrow] ||
                                [command.input isEqualToString:UIKeyInputDownArrow];
                if (!vertical || command.modifierFlags != 0) {
                    [kept addObject:command];
                }
            }
            return kept;
        });
        class_replaceMethod(cls, selector, filtered, method_getTypeEncoding(method));
    });
}

// Whether a hardware keyboard is connected now.
int32_t ff_touch_keyboard_connected(void) {
    if (@available(iOS 14.0, *)) {
        return GCKeyboard.coalescedKeyboard != nil;
    }
    return 0;
}
