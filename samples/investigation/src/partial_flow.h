namespace Game {
namespace Investigation {

struct PendingState {
    bool armed;
};

void CopyArmedFlag(PendingState* state, bool value);
bool ConsumeArmedFlag(PendingState* state);
void HandleFallback(PendingState* state, bool value);

} // namespace Investigation
} // namespace Game
