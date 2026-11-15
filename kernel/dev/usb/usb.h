// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

constexpr uint16_t USB_ANY_ID = 0xffff;
constexpr uint8_t USB_ANY_CLASS = 0xff;

struct usb_match {
    uint16_t vendor_id;
    uint16_t product_id;
    uint8_t class;
    uint8_t subclass;
    uint8_t protocol;
    uintptr_t data;
};

#define USB_MATCH_DEVICE(vendor, product) \
    .vendor_id = (vendor), .product_id = (product), .class = USB_ANY_CLASS, .subclass = USB_ANY_CLASS, \
    .protocol = USB_ANY_CLASS

#define USB_MATCH_CLASS(c, sc, p) \
    .vendor_id = USB_ANY_ID, .product_id = USB_ANY_ID, .class = (c), .subclass = (sc), .protocol = (p)
