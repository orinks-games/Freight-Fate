cmake_minimum_required(VERSION 3.26)
project(PrismSpeechBridge LANGUAGES Swift)
add_library(PrismSpeechBridge STATIC Sources/PrismWatchSpeech.swift)
set_target_properties(
  PrismSpeechBridge PROPERTIES Swift_MODULE_NAME PrismSpeechBridge
                               POSITION_INDEPENDENT_CODE ON)
set(PRISM_SWIFT_OBJC_HEADER_DIR "${CMAKE_CURRENT_BINARY_DIR}/include")
file(MAKE_DIRECTORY "${PRISM_SWIFT_OBJC_HEADER_DIR}")
target_compile_options(
  PrismSpeechBridge
  PRIVATE
    "$<$<COMPILE_LANGUAGE:Swift>:SHELL:-emit-objc-header -emit-objc-header-path ${PRISM_SWIFT_OBJC_HEADER_DIR}/PrismSpeechBridge-Swift.h>"
)
