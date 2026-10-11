// The iOS touch surface: one full-screen accessibility element over SDL's
// view, carrying the game's gesture recognizers.
//
// VoiceOver users meet a single element, "Freight Fate", that allows direct
// interaction (silent on touch), so the gestures below reach the game with
// VoiceOver on or off. The standard VoiceOver actions (double tap to
// activate, swipe up and down on an adjustable element, the two-finger
// scrub, the magic tap, the three-finger scroll) are answered too, for when
// direct touch is not active.
//
// The core rule: a flick is one step, a swipe and hold is continuous. A
// stroke that travels ff_flick_points and lifts within ff_flick_seconds is
// a flick; one still down after that is a hold in its direction (up gas,
// down brake, left and right steer). A swipe down ff_deep_factor times as
// far is the deep swipe (emergency or parking brake), and a finger held
// still for ff_still_seconds straightens the wheel. Two fingers held are the
// horn; turning them ff_rotate_degrees is the engine key, and the turn
// cancels the horn. A second finger tapping or flicking while a pedal is
// held is a gesture of its own. No screen position is used, only relative
// distances and times, so everything works anywhere on the screen.
// Every gesture is queued as a small integer code; the Rust side
// (`touch.rs`) drains the queue each frame and hands each gesture to the
// game. Speech never comes from here: Prism speaks through VoiceOver's
// announcement channel.

#import <UIKit/UIKit.h>
#import <UIKit/UIGestureRecognizerSubclass.h>
#import <GameController/GameController.h>
#import <objc/runtime.h>
#include <os/lock.h>
#include <stdint.h>
#include <math.h>

// Keep in step with `touch::Gesture::from_code`.
enum {
    FF_TAP = 0,
    FF_DOUBLE_TAP = 1,
    FF_SWIPE_UP = 2,
    FF_SWIPE_DOWN = 3,
    FF_SWIPE_LEFT = 4,
    FF_SWIPE_RIGHT = 5,
    FF_TWO_FINGER_TAP = 6,
    FF_TWO_FINGER_SWIPE_UP = 7,
    FF_TWO_FINGER_SWIPE_DOWN = 8,
    FF_TWO_FINGER_SWIPE_LEFT = 9,
    FF_TWO_FINGER_SWIPE_RIGHT = 10,
    FF_THREE_FINGER_SWIPE_UP = 11,
    FF_THREE_FINGER_SWIPE_DOWN = 12,
    // 13 was the three-finger double tap (on-screen keyboard); retired.
    FF_HOLD_UPPER_BEGAN = 14, // swipe up and hold: gas
    FF_HOLD_LOWER_BEGAN = 15, // swipe down and hold: brake
    FF_HOLD_ENDED = 16,
    FF_ESCAPE = 17,
    FF_MAGIC_TAP = 18,
    FF_ACTIVATE = 19,
    FF_INCREMENT = 20,
    FF_DECREMENT = 21,
    FF_THREE_FINGER_SWIPE_LEFT = 22,
    FF_THREE_FINGER_SWIPE_RIGHT = 23,
    FF_THREE_FINGER_TAP = 24,
    // A second finger while a pedal is held: the hold's base code plus one of
    // the FF_SECOND_* offsets below.
    FF_UPPER_HOLD_BASE = 25,
    FF_LOWER_HOLD_BASE = 31,
    FF_EMERGENCY_BRAKE_HOLD_BEGAN = 37, // no longer sent; the deep swipe took over
    FF_HORN_HOLD_BEGAN = 38,
    FF_HOLD_LEFT_BEGAN = 39,
    FF_HOLD_RIGHT_BEGAN = 40,
    FF_DEEP_SWIPE_DOWN_BEGAN = 41,
    FF_STILL_HOLD_BEGAN = 42,
    FF_SECOND_STEER_LEFT_BEGAN = 43,
    FF_SECOND_STEER_RIGHT_BEGAN = 44,
    FF_SECOND_STEER_ENDED = 45,
    FF_ROTATE_RIGHT = 46,
    FF_ROTATE_LEFT = 47,
    FF_KEYBOARD_CONNECTED = 48,
    FF_KEYBOARD_DISCONNECTED = 49,
};

enum {
    FF_SECOND_TAP = 0,
    FF_SECOND_DOUBLE_TAP = 1,
    FF_SECOND_SWIPE_UP = 2,
    FF_SECOND_SWIPE_DOWN = 3,
    FF_SECOND_SWIPE_LEFT = 4,
    FF_SECOND_SWIPE_RIGHT = 5,
};

#define FF_QUEUE_CAPACITY 64

static int32_t ff_queue[FF_QUEUE_CAPACITY];
static unsigned ff_head = 0;
static unsigned ff_len = 0;
static os_unfair_lock ff_lock = OS_UNFAIR_LOCK_INIT;
static BOOL ff_haptics_enabled = YES;
static UIImpactFeedbackGenerator *ff_impact;
static UISelectionFeedbackGenerator *ff_selection;
static UINotificationFeedbackGenerator *ff_notification;

void ff_touch_set_haptics(int enabled) {
    ff_haptics_enabled = enabled != 0;
    if (!ff_impact) {
        ff_impact = [[UIImpactFeedbackGenerator alloc] initWithStyle:UIImpactFeedbackStyleLight];
        ff_selection = [[UISelectionFeedbackGenerator alloc] init];
        ff_notification = [[UINotificationFeedbackGenerator alloc] init];
    }
    [ff_impact prepare];
    [ff_selection prepare];
    [ff_notification prepare];
}

// kind: 0 light impact, 1 selection, 2 warning, 3 success.
void ff_touch_haptic(int kind) {
    if (!ff_haptics_enabled) return;
    ff_touch_set_haptics(1);
    if (kind == 0) [ff_impact impactOccurred];
    else if (kind == 1) [ff_selection selectionChanged];
    else if (kind == 2) [ff_notification notificationOccurred:UINotificationFeedbackTypeWarning];
    else if (kind == 3) [ff_notification notificationOccurred:UINotificationFeedbackTypeSuccess];
}

// The haptic every action gets: a tick for one step, a bump for a hold
// starting or ending, a warning for the deep swipe, a success for the key.
static int ff_haptic_kind(int32_t code) {
    switch (code) {
    case FF_KEYBOARD_CONNECTED:
    case FF_KEYBOARD_DISCONNECTED:
        return -1;
    case FF_HOLD_UPPER_BEGAN:
    case FF_HOLD_LOWER_BEGAN:
    case FF_HOLD_LEFT_BEGAN:
    case FF_HOLD_RIGHT_BEGAN:
    case FF_STILL_HOLD_BEGAN:
    case FF_HORN_HOLD_BEGAN:
    case FF_SECOND_STEER_LEFT_BEGAN:
    case FF_SECOND_STEER_RIGHT_BEGAN:
    case FF_SECOND_STEER_ENDED:
    case FF_HOLD_ENDED:
        return 0;
    case FF_DEEP_SWIPE_DOWN_BEGAN:
    case FF_EMERGENCY_BRAKE_HOLD_BEGAN:
        return 2;
    case FF_ROTATE_RIGHT:
    case FF_ROTATE_LEFT:
        return 3;
    default:
        return 1;
    }
}

static void ff_push(int32_t code) {
    os_unfair_lock_lock(&ff_lock);
    if (ff_len == FF_QUEUE_CAPACITY) {
        // Drop the oldest: a stalled frame must not wedge the newest press.
        ff_head = (ff_head + 1) % FF_QUEUE_CAPACITY;
        ff_len--;
    }
    ff_queue[(ff_head + ff_len) % FF_QUEUE_CAPACITY] = code;
    ff_len++;
    os_unfair_lock_unlock(&ff_lock);
}

static void ff_emit(int32_t code) {
    ff_push(code);
    int kind = ff_haptic_kind(code);
    if (kind >= 0) ff_touch_haptic(kind);
}

int32_t ff_touch_next(void) {
    int32_t code = -1;
    os_unfair_lock_lock(&ff_lock);
    if (ff_len > 0) {
        code = ff_queue[ff_head];
        ff_head = (ff_head + 1) % FF_QUEUE_CAPACITY;
        ff_len--;
    }
    os_unfair_lock_unlock(&ff_lock);
    return code;
}

// Tunables. The defaults are the design's numbers; ff_touch_set_tuning
// replaces them from the player's settings.
static CGFloat ff_flick_points = 30.0;
static NSTimeInterval ff_flick_seconds = 0.15;
static CGFloat ff_deep_factor = 3.0;
static NSTimeInterval ff_still_seconds = 0.5;
static CGFloat ff_rotate_degrees = 30.0;

static const NSTimeInterval FF_HOLD_SECONDS = 0.35;
static const NSTimeInterval FF_DOUBLE_TAP_SECONDS = 0.28;
static const NSTimeInterval FF_SECOND_TAP_MAX_SECONDS = 0.4;
static const CGFloat FF_HOLD_SLOP = 12.0;
static const CGFloat FF_TAP_SLOP = 16.0;
// A finger still travelling when the flick window ends is mid-flick, not
// holding: it gets up to FF_FLICK_EXTENSIONS more half windows while it keeps
// moving at least this fraction of ff_flick_points per half window.
static const CGFloat FF_FLICK_MOVING_FRACTION = 0.4;
static const int FF_FLICK_EXTENSIONS = 3;

void ff_touch_set_tuning(double flick_points, double flick_ms, double deep_factor,
                         double still_ms, double rotate_degrees) {
    ff_flick_points = fmax(10.0, fmin(120.0, flick_points));
    ff_flick_seconds = fmax(0.05, fmin(0.6, flick_ms / 1000.0));
    ff_deep_factor = fmax(1.5, fmin(6.0, deep_factor));
    ff_still_seconds = fmax(0.2, fmin(2.0, still_ms / 1000.0));
    ff_rotate_degrees = fmax(10.0, fmin(90.0, rotate_degrees));
}

int32_t ff_touch_keyboard_connected(void) {
    if (@available(iOS 14.0, *)) {
        return GCKeyboard.coalescedKeyboard != nil;
    }
    return 0;
}

typedef NS_ENUM(NSInteger, FFDirection) { FFNone, FFUp, FFDown, FFLeft, FFRight };

static FFDirection ff_direction(CGFloat dx, CGFloat dy) {
    if (fabs(dx) > fabs(dy)) return dx > 0 ? FFRight : FFLeft;
    return dy > 0 ? FFDown : FFUp;
}

// One finger's stroke. Once the finger has travelled ff_flick_points, it
// has ff_flick_seconds to lift (a flick, one step) before the stroke becomes
// a hold in that direction (continuous until it lifts). While gas or brake
// is held, a second finger's taps, flicks and held side swipes are read here
// too, since the recognizer that owns the held touch is the one UIKit keeps
// feeding new touches to. A second tap waits FF_DOUBLE_TAP_SECONDS for a
// partner before it counts as a single tap, and a pending tap is sent before
// the pedal lets go, so "hold, tap, lift" runs the tap with the pedal down.

@interface FFStrokeRecognizer : UIGestureRecognizer
@end

@implementation FFStrokeRecognizer {
    UITouch *_first;
    CGPoint _start;
    FFDirection _direction;
    NSTimer *_flickTimer;
    CGPoint _flickSample;
    int _flickExtensions;
    NSTimer *_stillTimer;
    int32_t _hold;
    UITouch *_second;
    CGPoint _secondStart;
    NSTimeInterval _secondStartTime;
    FFDirection _secondDirection;
    NSTimer *_secondHoldTimer;
    BOOL _secondSteering;
    NSTimer *_tapTimer;
    int32_t _tapBase;
}

- (int32_t)pedalBase {
    if (_hold == FF_HOLD_UPPER_BEGAN) return FF_UPPER_HOLD_BASE;
    if (_hold == FF_HOLD_LOWER_BEGAN) return FF_LOWER_HOLD_BASE;
    return -1;
}

- (void)touchesBegan:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event {
    for (UITouch *touch in touches) {
        if (!_first) {
            _first = touch;
            _start = [touch locationInView:self.view];
            _direction = FFNone;
            _stillTimer = [NSTimer scheduledTimerWithTimeInterval:ff_still_seconds
                                                           target:self
                                                         selector:@selector(stillElapsed)
                                                         userInfo:nil
                                                          repeats:NO];
        } else if (self.state == UIGestureRecognizerStatePossible) {
            // Two fingers down together are a two-finger gesture, not a stroke.
            self.state = UIGestureRecognizerStateFailed;
            return;
        } else if (!_second && [self pedalBase] >= 0) {
            _second = touch;
            _secondStart = [touch locationInView:self.view];
            _secondStartTime = touch.timestamp;
            _secondDirection = FFNone;
            _secondSteering = NO;
        }
    }
}

- (void)touchesMoved:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event {
    if (_first && [touches containsObject:_first]) {
        [self firstMoved];
    }
    if (_second && [touches containsObject:_second]) {
        [self secondMoved];
    }
}

- (void)firstMoved {
    CGPoint at = [_first locationInView:self.view];
    CGFloat dx = at.x - _start.x;
    CGFloat dy = at.y - _start.y;
    CGFloat distance = hypot(dx, dy);
    if (_stillTimer && distance > FF_HOLD_SLOP) {
        [_stillTimer invalidate];
        _stillTimer = nil;
    }
    if (_direction == FFNone && distance >= ff_flick_points) {
        if (self.state != UIGestureRecognizerStatePossible) {
            return; // a still hold already straightening keeps going
        }
        _direction = ff_direction(dx, dy);
        _flickSample = at;
        _flickExtensions = 0;
        _flickTimer = [NSTimer scheduledTimerWithTimeInterval:ff_flick_seconds
                                                       target:self
                                                     selector:@selector(flickElapsed)
                                                     userInfo:nil
                                                      repeats:NO];
    }
    // Only a stroke that has outlasted the flick window (the down hold has
    // begun) can become the deep swipe; a fast long flick down stays a flick.
    if (_direction == FFDown && _hold == FF_HOLD_LOWER_BEGAN &&
        dy >= ff_flick_points * ff_deep_factor) {
        [_flickTimer invalidate];
        _flickTimer = nil;
        [self flushPendingTap];
        [self endSecond];
        BOOL starting = self.state == UIGestureRecognizerStatePossible;
        _hold = FF_DEEP_SWIPE_DOWN_BEGAN;
        ff_emit(FF_DEEP_SWIPE_DOWN_BEGAN);
        self.state = starting ? UIGestureRecognizerStateBegan : UIGestureRecognizerStateChanged;
    }
}

- (void)flickElapsed {
    _flickTimer = nil;
    if (self.state != UIGestureRecognizerStatePossible || !_first || _direction == FFNone) {
        return;
    }
    // The window starts when the finger crosses ff_flick_points, so a long,
    // brisk flick is still on its way when it ends. Judge by whether the
    // finger is still travelling, not by the clock alone: only a stroke that
    // has slowed or stopped becomes a hold. Without this, a flick up with
    // cruise on turned into a gas hold and never stepped the target.
    CGPoint at = [_first locationInView:self.view];
    CGFloat moved = hypot(at.x - _flickSample.x, at.y - _flickSample.y);
    if (_flickExtensions < FF_FLICK_EXTENSIONS &&
        moved >= ff_flick_points * FF_FLICK_MOVING_FRACTION) {
        _flickExtensions++;
        _flickSample = at;
        _flickTimer = [NSTimer scheduledTimerWithTimeInterval:ff_flick_seconds / 2.0
                                                       target:self
                                                     selector:@selector(flickElapsed)
                                                     userInfo:nil
                                                      repeats:NO];
        return;
    }
    switch (_direction) {
    case FFUp: _hold = FF_HOLD_UPPER_BEGAN; break;
    case FFDown: _hold = FF_HOLD_LOWER_BEGAN; break;
    case FFLeft: _hold = FF_HOLD_LEFT_BEGAN; break;
    default: _hold = FF_HOLD_RIGHT_BEGAN; break;
    }
    ff_emit(_hold);
    self.state = UIGestureRecognizerStateBegan;
}

- (void)stillElapsed {
    _stillTimer = nil;
    if (self.state != UIGestureRecognizerStatePossible || !_first || _direction != FFNone) {
        return;
    }
    _hold = FF_STILL_HOLD_BEGAN;
    ff_emit(FF_STILL_HOLD_BEGAN);
    self.state = UIGestureRecognizerStateBegan;
}

- (void)touchesEnded:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event {
    if (_second && [touches containsObject:_second]) {
        [self secondLifted];
    }
    if (_first && [touches containsObject:_first]) {
        if (_hold) {
            [self flushPendingTap];
            [self endSecond];
            ff_emit(FF_HOLD_ENDED);
            self.state = UIGestureRecognizerStateEnded;
        } else if (_direction != FFNone && self.state == UIGestureRecognizerStatePossible) {
            int32_t code = FF_SWIPE_RIGHT;
            if (_direction == FFUp) code = FF_SWIPE_UP;
            else if (_direction == FFDown) code = FF_SWIPE_DOWN;
            else if (_direction == FFLeft) code = FF_SWIPE_LEFT;
            ff_emit(code);
            self.state = UIGestureRecognizerStateEnded;
        } else {
            self.state = UIGestureRecognizerStateFailed;
        }
    }
}

- (void)touchesCancelled:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event {
    if (_second && [touches containsObject:_second]) {
        [self endSecond];
    }
    if (_first && [touches containsObject:_first]) {
        if (_hold) {
            [self endSecond];
            ff_emit(FF_HOLD_ENDED);
            self.state = UIGestureRecognizerStateCancelled;
        } else {
            self.state = UIGestureRecognizerStateFailed;
        }
    }
}

- (void)secondMoved {
    if ([self pedalBase] < 0 || _secondDirection != FFNone) {
        return;
    }
    CGPoint at = [_second locationInView:self.view];
    CGFloat dx = at.x - _secondStart.x;
    CGFloat dy = at.y - _secondStart.y;
    if (hypot(dx, dy) < ff_flick_points) {
        return;
    }
    _secondDirection = ff_direction(dx, dy);
    if (_secondDirection == FFLeft || _secondDirection == FFRight) {
        _secondHoldTimer = [NSTimer scheduledTimerWithTimeInterval:ff_flick_seconds
                                                            target:self
                                                          selector:@selector(secondHoldElapsed)
                                                          userInfo:nil
                                                           repeats:NO];
    }
}

- (void)secondHoldElapsed {
    _secondHoldTimer = nil;
    if (!_second || [self pedalBase] < 0) {
        return;
    }
    [self flushPendingTap];
    _secondSteering = YES;
    ff_emit(_secondDirection == FFLeft ? FF_SECOND_STEER_LEFT_BEGAN : FF_SECOND_STEER_RIGHT_BEGAN);
    self.state = UIGestureRecognizerStateChanged;
}

// Let go of the second finger's steering, if it is steering.
- (void)endSecond {
    [_secondHoldTimer invalidate];
    _secondHoldTimer = nil;
    if (_secondSteering) {
        _secondSteering = NO;
        ff_emit(FF_SECOND_STEER_ENDED);
    }
    _second = nil;
}

- (void)secondLifted {
    UITouch *touch = _second;
    if (_secondSteering) {
        [self endSecond];
        self.state = UIGestureRecognizerStateChanged;
        return;
    }
    [self endSecond];
    int32_t base = [self pedalBase];
    if (base < 0) {
        return;
    }
    CGPoint at = [touch locationInView:self.view];
    CGFloat dx = at.x - _secondStart.x;
    CGFloat dy = at.y - _secondStart.y;
    CGFloat distance = hypot(dx, dy);
    if (_secondDirection != FFNone || distance >= ff_flick_points) {
        [self flushPendingTap];
        int32_t offset;
        switch (ff_direction(dx, dy)) {
        case FFUp: offset = FF_SECOND_SWIPE_UP; break;
        case FFDown: offset = FF_SECOND_SWIPE_DOWN; break;
        case FFLeft: offset = FF_SECOND_SWIPE_LEFT; break;
        default: offset = FF_SECOND_SWIPE_RIGHT; break;
        }
        ff_emit(base + offset);
    } else if (distance <= FF_TAP_SLOP && touch.timestamp - _secondStartTime <= FF_SECOND_TAP_MAX_SECONDS) {
        if (_tapTimer) {
            [_tapTimer invalidate];
            _tapTimer = nil;
            ff_emit(base + FF_SECOND_DOUBLE_TAP);
        } else {
            _tapBase = base;
            _tapTimer = [NSTimer scheduledTimerWithTimeInterval:FF_DOUBLE_TAP_SECONDS
                                                         target:self
                                                       selector:@selector(tapElapsed)
                                                       userInfo:nil
                                                        repeats:NO];
        }
    } else {
        return;
    }
    self.state = UIGestureRecognizerStateChanged;
}

- (void)tapElapsed {
    _tapTimer = nil;
    ff_emit(_tapBase + FF_SECOND_TAP);
}

- (void)flushPendingTap {
    if (_tapTimer) {
        [_tapTimer invalidate];
        [self tapElapsed];
    }
}

- (void)reset {
    [super reset];
    [_flickTimer invalidate];
    _flickTimer = nil;
    _flickExtensions = 0;
    [_stillTimer invalidate];
    _stillTimer = nil;
    [_secondHoldTimer invalidate];
    _secondHoldTimer = nil;
    [_tapTimer invalidate];
    _tapTimer = nil;
    _first = nil;
    _second = nil;
    _hold = 0;
    _direction = FFNone;
    _secondDirection = FFNone;
    _secondSteering = NO;
}

@end

@interface FFTouchView : UIView <UIGestureRecognizerDelegate>
@end

@implementation FFTouchView {
    UILongPressGestureRecognizer *_twoHold;
    UIRotationGestureRecognizer *_rotation;
    BOOL _rotationFired;
    BOOL _hornCancelled;
}

- (instancetype)initWithFrame:(CGRect)frame {
    self = [super initWithFrame:frame];
    if (!self) {
        return nil;
    }
    self.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    self.backgroundColor = UIColor.clearColor;
    self.multipleTouchEnabled = YES;
    self.isAccessibilityElement = YES;
    self.accessibilityLabel = @"Freight Fate";
    self.accessibilityHint = @"A flick is one step, a swipe and hold is continuous. Swipe up and hold "
                             @"for gas, swipe down and hold to brake, anywhere on the screen.";
    self.accessibilityTraits =
        UIAccessibilityTraitAllowsDirectInteraction | UIAccessibilityTraitAdjustable;
    if (@available(iOS 17.0, *)) {
        self.accessibilityDirectTouchOptions = UIAccessibilityDirectTouchOptionSilentOnTouch;
    }
    NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
    [center addObserver:self
               selector:@selector(voiceOverChanged:)
                   name:UIAccessibilityVoiceOverStatusDidChangeNotification
                 object:nil];
    if (@available(iOS 14.0, *)) {
        [center addObserver:self
                   selector:@selector(keyboardChanged:)
                       name:GCKeyboardDidConnectNotification
                     object:nil];
        [center addObserver:self
                   selector:@selector(keyboardChanged:)
                       name:GCKeyboardDidDisconnectNotification
                     object:nil];
    }
    ff_push(ff_touch_keyboard_connected() ? FF_KEYBOARD_CONNECTED : FF_KEYBOARD_DISCONNECTED);
    [self installRecognizers];
    return self;
}

- (void)installRecognizers {
    [self tapWithTouches:3 taps:1 code:FF_THREE_FINGER_TAP];
    UITapGestureRecognizer *twoDouble = [self tapWithTouches:2 taps:2 code:FF_MAGIC_TAP];
    UITapGestureRecognizer *twoSingle = [self tapWithTouches:2 taps:1 code:FF_TWO_FINGER_TAP];
    [twoSingle requireGestureRecognizerToFail:twoDouble];
    UITapGestureRecognizer *oneDouble = [self tapWithTouches:1 taps:2 code:FF_DOUBLE_TAP];
    UITapGestureRecognizer *oneSingle = [self tapWithTouches:1 taps:1 code:FF_TAP];
    [oneSingle requireGestureRecognizerToFail:oneDouble];

    const struct {
        NSUInteger touches;
        UISwipeGestureRecognizerDirection direction;
        int32_t code;
    } swipes[] = {
        {2, UISwipeGestureRecognizerDirectionUp, FF_TWO_FINGER_SWIPE_UP},
        {2, UISwipeGestureRecognizerDirectionDown, FF_TWO_FINGER_SWIPE_DOWN},
        {2, UISwipeGestureRecognizerDirectionLeft, FF_TWO_FINGER_SWIPE_LEFT},
        {2, UISwipeGestureRecognizerDirectionRight, FF_TWO_FINGER_SWIPE_RIGHT},
        {3, UISwipeGestureRecognizerDirectionUp, FF_THREE_FINGER_SWIPE_UP},
        {3, UISwipeGestureRecognizerDirectionDown, FF_THREE_FINGER_SWIPE_DOWN},
        {3, UISwipeGestureRecognizerDirectionLeft, FF_THREE_FINGER_SWIPE_LEFT},
        {3, UISwipeGestureRecognizerDirectionRight, FF_THREE_FINGER_SWIPE_RIGHT},
    };
    for (size_t i = 0; i < sizeof(swipes) / sizeof(swipes[0]); i++) {
        UISwipeGestureRecognizer *swipe =
            [[UISwipeGestureRecognizer alloc] initWithTarget:self action:@selector(swiped:)];
        swipe.numberOfTouchesRequired = swipes[i].touches;
        swipe.direction = swipes[i].direction;
        [swipe setValue:@(swipes[i].code) forKey:@"ffCode"];
        [self addGestureRecognizer:swipe];
    }

    // One finger's flicks and holds all come from the stroke recognizer.
    FFStrokeRecognizer *stroke = [[FFStrokeRecognizer alloc] initWithTarget:nil action:nil];
    [oneSingle requireGestureRecognizerToFail:stroke];
    [self addGestureRecognizer:stroke];

    _twoHold = [self addHoldWithTouches:2 code:FF_HORN_HOLD_BEGAN];
    _twoHold.delegate = self;
    [self addHoldWithTouches:3 code:FF_HORN_HOLD_BEGAN];
    _rotation = [[UIRotationGestureRecognizer alloc] initWithTarget:self action:@selector(rotated:)];
    _rotation.delegate = self;
    [self addGestureRecognizer:_rotation];
}

// The horn and the key turn share two fingers: a turn starting while the
// horn sounds has to be seen, so it can cancel the horn.
- (BOOL)gestureRecognizer:(UIGestureRecognizer *)first
    shouldRecognizeSimultaneouslyWithGestureRecognizer:(UIGestureRecognizer *)second {
    return (first == _rotation && second == _twoHold) || (first == _twoHold && second == _rotation);
}

- (UILongPressGestureRecognizer *)addHoldWithTouches:(NSUInteger)touches code:(int32_t)code {
    UILongPressGestureRecognizer *hold = [[UILongPressGestureRecognizer alloc] initWithTarget:self action:@selector(held:)];
    hold.numberOfTouchesRequired = touches;
    hold.minimumPressDuration = FF_HOLD_SECONDS;
    hold.allowableMovement = FF_HOLD_SLOP;
    [hold setValue:@(code) forKey:@"ffCode"];
    [self addGestureRecognizer:hold];
    return hold;
}

- (void)held:(UILongPressGestureRecognizer *)hold {
    int32_t code = [[hold valueForKey:@"ffCode"] intValue];
    if (hold.state == UIGestureRecognizerStateBegan) {
        if (hold == _twoHold) {
            _hornCancelled = NO;
        }
        ff_emit(code);
    } else if (hold.state == UIGestureRecognizerStateEnded || hold.state == UIGestureRecognizerStateCancelled) {
        if (hold == _twoHold && _hornCancelled) {
            _hornCancelled = NO; // the turn already ended it
            return;
        }
        ff_emit(FF_HOLD_ENDED);
    }
}

// Two fingers turning like a key: right (clockwise) starts the engine, left
// stops it, once per turn. A third of the way there the horn is let go.
- (void)rotated:(UIRotationGestureRecognizer *)rotation {
    if (rotation.state == UIGestureRecognizerStateBegan) {
        _rotationFired = NO;
    }
    if (rotation.state != UIGestureRecognizerStateBegan && rotation.state != UIGestureRecognizerStateChanged) {
        return;
    }
    CGFloat degrees = fabs(rotation.rotation) * 180.0 / M_PI;
    BOOL horn = _twoHold.state == UIGestureRecognizerStateBegan || _twoHold.state == UIGestureRecognizerStateChanged;
    if (horn && !_hornCancelled && degrees >= ff_rotate_degrees / 3.0) {
        _hornCancelled = YES;
        ff_emit(FF_HOLD_ENDED);
        _twoHold.enabled = NO;
        _twoHold.enabled = YES;
    }
    if (!_rotationFired && degrees >= ff_rotate_degrees) {
        _rotationFired = YES;
        ff_emit(rotation.rotation > 0 ? FF_ROTATE_RIGHT : FF_ROTATE_LEFT);
    }
}

- (void)voiceOverChanged:(NSNotification *)notification {
    UIAccessibilityPostNotification(UIAccessibilityScreenChangedNotification, self);
}

- (void)keyboardChanged:(NSNotification *)notification {
    BOOL connected = ff_touch_keyboard_connected();
    if (@available(iOS 14.0, *)) {
        if ([notification.name isEqualToString:GCKeyboardDidConnectNotification]) {
            connected = YES;
        }
    }
    ff_push(connected ? FF_KEYBOARD_CONNECTED : FF_KEYBOARD_DISCONNECTED);
}

- (UITapGestureRecognizer *)tapWithTouches:(NSUInteger)touches taps:(NSUInteger)taps code:(int32_t)code {
    UITapGestureRecognizer *tap =
        [[UITapGestureRecognizer alloc] initWithTarget:self action:@selector(tapped:)];
    tap.numberOfTouchesRequired = touches;
    tap.numberOfTapsRequired = taps;
    [tap setValue:@(code) forKey:@"ffCode"];
    [self addGestureRecognizer:tap];
    return tap;
}

- (void)tapped:(UITapGestureRecognizer *)tap {
    if (tap.state == UIGestureRecognizerStateEnded) {
        ff_emit([[tap valueForKey:@"ffCode"] intValue]);
    }
}

- (void)swiped:(UISwipeGestureRecognizer *)swipe {
    if (swipe.state == UIGestureRecognizerStateEnded) {
        ff_emit([[swipe valueForKey:@"ffCode"] intValue]);
    }
}

- (BOOL)accessibilityActivate {
    ff_emit(FF_ACTIVATE);
    return YES;
}

- (void)accessibilityIncrement {
    ff_emit(FF_INCREMENT);
}

- (void)accessibilityDecrement {
    ff_emit(FF_DECREMENT);
}

- (BOOL)accessibilityPerformEscape {
    ff_emit(FF_ESCAPE);
    return YES;
}

- (BOOL)accessibilityPerformMagicTap {
    ff_emit(FF_MAGIC_TAP);
    return YES;
}

- (BOOL)accessibilityScroll:(UIAccessibilityScrollDirection)direction {
    switch (direction) {
    case UIAccessibilityScrollDirectionLeft:
        ff_emit(FF_THREE_FINGER_SWIPE_LEFT);
        return YES;
    case UIAccessibilityScrollDirectionRight:
        ff_emit(FF_THREE_FINGER_SWIPE_RIGHT);
        return YES;
    case UIAccessibilityScrollDirectionUp:
        ff_emit(FF_THREE_FINGER_SWIPE_UP);
        return YES;
    case UIAccessibilityScrollDirectionDown:
        ff_emit(FF_THREE_FINGER_SWIPE_DOWN);
        return YES;
    default:
        return NO;
    }
}

@end

// The recognizers carry their gesture code as an associated value.

static const void *FF_CODE_KEY = &FF_CODE_KEY;

@interface UIGestureRecognizer (FFCode)
@end

@implementation UIGestureRecognizer (FFCode)
- (void)setFfCode:(NSNumber *)code {
    objc_setAssociatedObject(self, FF_CODE_KEY, code, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}
- (NSNumber *)ffCode {
    return objc_getAssociatedObject(self, FF_CODE_KEY);
}
@end

// Ask iOS to defer its edge gestures (Home, Control Center, Notification
// Center) to a second swipe, so a stroke that starts at an edge still
// reaches the game. SDL's controller answers this from a hint; ours always
// answers every edge.
static void ff_defer_screen_edges(UIViewController *controller) {
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

// Lay the touch surface over SDL's view. `window` is the UIWindow SDL made
// (what its window-manager info reports); a UIView is accepted as well.
// Returns 1 when the surface is in place.
int32_t ff_touch_install(void *window) {
    if (!window) {
        return 0;
    }
    id object = (__bridge id)window;
    UIView *host = nil;
    if ([object isKindOfClass:[UIWindow class]]) {
        UIWindow *ui_window = (UIWindow *)object;
        host = ui_window.rootViewController.view ?: ui_window;
    } else if ([object isKindOfClass:[UIView class]]) {
        host = (UIView *)object;
    }
    if (!host) {
        return 0;
    }
    FFTouchView *surface = [[FFTouchView alloc] initWithFrame:host.bounds];
    [host addSubview:surface];
    ff_defer_screen_edges(host.window.rootViewController);
    UIAccessibilityPostNotification(UIAccessibilityScreenChangedNotification, surface);
    return 1;
}
