#include <vector>
#include "local.hpp"

#define VERSION 2

using namespace std;
using std::vector;
using Alias = int;

namespace app {
/** A widget. */
template <typename T>
class Widget {
public:
    T value;
    /** Resets the widget. */
    void reset() { value = T(); }
};
}

/** Computes the sum. */
template <typename T>
T sum(T a, T b) {
    return a + b;
}
