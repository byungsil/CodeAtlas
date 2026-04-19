namespace Game {
namespace Investigation {

struct ShotRequest {
    int power;
    bool armed;
};

struct EventHint {
    int power;
};

struct EventEnvelope {
    EventHint hint;
};

struct NestedEnvelope {
    EventEnvelope envelope;
};

class ShotController {
public:
    void LoadRequest(const ShotRequest& request);
    void LaunchIfReady();

private:
    int queuedPower;
    bool queuedArmed;
};

class HintController {
public:
    void ApplyHint(const EventHint& hint);
    void EmitHint();

private:
    int hintedPower;
};

class ConstructedHintController {
public:
    explicit ConstructedHintController(int initialPower);
    void EmitConstructed();

private:
    int seededPower;
};

class NestedConstructedHintController {
public:
    explicit NestedConstructedHintController(int initialPower);
    void EmitNestedConstructed();

private:
    int seededPower;
};

class RelayFieldController {
public:
    void ApplyHint(const EventHint& hint);
    void EmitStored();

private:
    int storedPower;
};

int ReadInputPower();
bool ReadInputArmed();
ShotRequest BuildShotRequest(int power, bool armed);
EventHint MakeHint(int power);
EventEnvelope MakeEnvelope(int power);
NestedEnvelope MakeNestedEnvelope(int power);
int ExtractHintPower(const EventHint& hint);
void QueueShot(ShotController& controller);
void RunHintWorkflow(HintController& controller);
void RunConstructedHintWorkflow();
void RunNestedHintWorkflow(HintController& controller);
void RunNestedConstructedHintWorkflow();
void RunRelayFieldWorkflow(RelayFieldController& controller);
void RunNestedRelayWorkflow();
void RunNestedRelayToForwarderWorkflow();
void LaunchShot(int power);
void LaunchHint(int power);
void EmitRelayHint(int power);
void EmitForwardedPower(int power);

} // namespace Investigation
} // namespace Game
