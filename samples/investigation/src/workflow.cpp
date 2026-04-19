#include "workflow.h"

namespace Game {
namespace Investigation {

int ReadInputPower() {
    return 42;
}

bool ReadInputArmed() {
    return true;
}

ShotRequest BuildShotRequest(int power, bool armed) {
    ShotRequest request{power, armed};
    return request;
}

EventHint MakeHint(int power) {
    EventHint hint{power};
    return hint;
}

EventEnvelope MakeEnvelope(int power) {
    EventEnvelope envelope{MakeHint(power)};
    return envelope;
}

NestedEnvelope MakeNestedEnvelope(int power) {
    NestedEnvelope nested{MakeEnvelope(power)};
    return nested;
}

int ExtractHintPower(const EventHint& hint) {
    return hint.power;
}

void ShotController::LoadRequest(const ShotRequest& request) {
    this->queuedPower = request.power;
    this->queuedArmed = request.armed;
}

void LaunchShot(int power) {
    (void)power;
}

void ShotController::LaunchIfReady() {
    if (this->queuedArmed) {
        int launchPower = this->queuedPower;
        LaunchShot(launchPower);
    }
}

void HintController::ApplyHint(const EventHint& hint) {
    this->hintedPower = hint.power;
}

void LaunchHint(int power) {
    (void)power;
}

void EmitRelayHint(int power) {
    LaunchHint(power);
}

void EmitForwardedPower(int power) {
    EmitRelayHint(power);
}

void HintController::EmitHint() {
    int launchPower = this->hintedPower;
    LaunchHint(launchPower);
}

void RelayFieldController::ApplyHint(const EventHint& hint) {
    this->storedPower = hint.power;
}

void RelayFieldController::EmitStored() {
    EmitRelayHint(this->storedPower);
}

ConstructedHintController::ConstructedHintController(int initialPower)
    : seededPower(MakeHint(initialPower).power) {}

void ConstructedHintController::EmitConstructed() {
    int launchPower = this->seededPower;
    LaunchHint(launchPower);
}

NestedConstructedHintController::NestedConstructedHintController(int initialPower)
    : seededPower(MakeNestedEnvelope(initialPower).envelope.hint.power) {}

void NestedConstructedHintController::EmitNestedConstructed() {
    int launchPower = this->seededPower;
    LaunchHint(launchPower);
}

void QueueShot(ShotController& controller) {
    int inputPower = ReadInputPower();
    bool armed = ReadInputArmed();
    ShotRequest request = BuildShotRequest(inputPower, armed);
    controller.LoadRequest(request);
    controller.LaunchIfReady();
}

void RunHintWorkflow(HintController& controller) {
    EventHint hint = MakeHint(ReadInputPower());
    controller.ApplyHint(hint);
    controller.EmitHint();
}

void RunNestedHintWorkflow(HintController& controller) {
    NestedEnvelope nested = MakeNestedEnvelope(ReadInputPower());
    controller.ApplyHint(nested.envelope.hint);
    controller.EmitHint();
}

void RunConstructedHintWorkflow() {
    ConstructedHintController controller(ReadInputPower());
    controller.EmitConstructed();
}

void RunNestedConstructedHintWorkflow() {
    NestedConstructedHintController controller(ReadInputPower());
    controller.EmitNestedConstructed();
}

void RunRelayFieldWorkflow(RelayFieldController& controller) {
    EventHint hint = MakeHint(ReadInputPower());
    controller.ApplyHint(hint);
    controller.EmitStored();
}

void RunNestedRelayWorkflow() {
    NestedEnvelope nested = MakeNestedEnvelope(ReadInputPower());
    int power = ExtractHintPower(nested.envelope.hint);
    EmitRelayHint(power);
}

void RunNestedRelayToForwarderWorkflow() {
    NestedEnvelope nested = MakeNestedEnvelope(ReadInputPower());
    int power = ExtractHintPower(nested.envelope.hint);
    EmitForwardedPower(power);
}

} // namespace Investigation
} // namespace Game
