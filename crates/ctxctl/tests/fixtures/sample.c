#include <stdio.h>
#include "util.h"

#define MAX_RETRIES 3

/** A 2D point. */
typedef struct Point {
    double x;
    double y;
} Point;

/** Adds two numbers. */
int add(int a, int b) {
    return a + b;
}

enum Color { RED, GREEN };

static int helper(int v) {
    return v * 2;
}
