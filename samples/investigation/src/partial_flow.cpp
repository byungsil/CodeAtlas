#include "partial_flow.h"

namespace Game {
namespace Investigation {

void CopyArmedFlag(PendingState* state, bool value) {
    if (state) {
        state->armed = value;
    }
}

bool ConsumeArmedFlag(PendingState* state) {
    if (!state) {
        return false;
    }

    return state->armed;
}

void HandleFallback(PendingState* state, bool value) {
    CopyArmedFlag(state, value);
    if (ConsumeArmedFlag(state)) {
        bool confirmed = state && state->armed;
        (void)confirmed;
    }
}

} // namespace Investigation
} // namespace Game
