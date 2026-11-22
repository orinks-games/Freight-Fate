// Raw fingers for the shared gesture recognizer (Touch gestures, New gesture
// reading).
//
// With the setting off, the touch surface's own UIKit recognizers
// (ff_touch.m) read the gestures and this queue stays empty. With it on,
// every recognizer on the surface is disabled, and each finger event the
// surface receives is queued here: a finger id that stays the same from
// down to up, where it is in points, the event's own UITouch.timestamp, and
// its kind. The game drains the queue each frame into
// `ff_core::touch::recognizer` (`src/app/sdl_shell/ios.rs`), which ticks on
// the same clock (ff_fingers_now), so a slow frame cannot turn a flick into
// a hold. VoiceOver's own actions (activate, escape, the magic tap, the
// adjustable swipes, the three-finger scroll) are not finger events: they
// keep arriving through ff_touch.m's codes either way.

#import <QuartzCore/QuartzCore.h>
#import <UIKit/UIKit.h>
#include <os/lock.h>
#include <stdint.h>

// Keep in step with `FingerKind::from_code`.
enum {
    FF_FINGER_DOWN = 0,
    FF_FINGER_MOVE = 1,
    FF_FINGER_UP = 2,
    FF_FINGER_CANCEL = 3,
};

typedef struct {
    int64_t finger;
    int32_t kind;
    float x;
    float y;
    double t;
} FFFinger;

// About two seconds of four fingers moving at 60 events a second; the game
// drains it every frame.
#define FF_FINGER_CAPACITY 512

static FFFinger ff_fingers[FF_FINGER_CAPACITY];
static unsigned ff_finger_head = 0;
static unsigned ff_finger_len = 0;
static os_unfair_lock ff_finger_lock = OS_UNFAIR_LOCK_INIT;
static BOOL ff_shared = NO;
static __weak UIView *ff_surface;

// The native recognizers run only while the shared one is off.
static void ff_fingers_apply(void) {
    UIView *surface = ff_surface;
    for (UIGestureRecognizer *recognizer in surface.gestureRecognizers) {
        recognizer.enabled = !ff_shared;
    }
}

// The touch surface, once it is in place (ff_touch_install).
void ff_fingers_attach(UIView *surface) {
    ff_surface = surface;
    ff_fingers_apply();
}

// Turn the shared recognizer on or off. Called on the main thread.
void ff_fingers_set_shared(int32_t on) {
    ff_shared = on != 0;
    os_unfair_lock_lock(&ff_finger_lock);
    ff_finger_len = 0;
    os_unfair_lock_unlock(&ff_finger_lock);
    ff_fingers_apply();
}

// Queue one record per touch, from the surface's touchesBegan, Moved, Ended
// and Cancelled.
void ff_fingers_touches(NSSet<UITouch *> *touches, UIView *view, int32_t kind) {
    if (!ff_shared) {
        return;
    }
    os_unfair_lock_lock(&ff_finger_lock);
    for (UITouch *touch in touches) {
        CGPoint at = [touch locationInView:view];
        if (ff_finger_len == FF_FINGER_CAPACITY) {
            // Drop the oldest: a stalled frame must not wedge the newest.
            ff_finger_head = (ff_finger_head + 1) % FF_FINGER_CAPACITY;
            ff_finger_len--;
        }
        ff_fingers[(ff_finger_head + ff_finger_len) % FF_FINGER_CAPACITY] = (FFFinger){
            // A UITouch is the same object from its first event to its last.
            .finger = (int64_t)(intptr_t)(__bridge void *)touch,
            .kind = kind,
            .x = (float)at.x,
            .y = (float)at.y,
            .t = touch.timestamp,
        };
        ff_finger_len++;
    }
    os_unfair_lock_unlock(&ff_finger_lock);
}

// The oldest queued record; returns 0 when the queue is empty.
int32_t ff_fingers_next(int64_t *finger, int32_t *kind, float *x, float *y, double *t) {
    int32_t found = 0;
    os_unfair_lock_lock(&ff_finger_lock);
    if (ff_finger_len > 0) {
        FFFinger record = ff_fingers[ff_finger_head];
        ff_finger_head = (ff_finger_head + 1) % FF_FINGER_CAPACITY;
        ff_finger_len--;
        *finger = record.finger;
        *kind = record.kind;
        *x = record.x;
        *y = record.y;
        *t = record.t;
        found = 1;
    }
    os_unfair_lock_unlock(&ff_finger_lock);
    return found;
}

// Now, on UITouch.timestamp's clock (both count from boot, not in sleep).
double ff_fingers_now(void) {
    return CACurrentMediaTime();
}
