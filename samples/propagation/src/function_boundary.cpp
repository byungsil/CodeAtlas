namespace Game {
void Consume(int value) {}

int Forward(int value) {
    int local = value;
    return local;
}

struct EventHint {
    int power;
};

EventHint MakeHint(int value) {
    EventHint hint{value};
    return hint;
}

class BoundaryWorker {
public:
    void ApplyHint(const EventHint& hint) {
        this->stored = hint.power;
    }

    void Run(int source) {
        EventHint staged = MakeHint(source);
        ApplyHint(staged);
    }

private:
    int stored;
};

void Tick(int source) {
    int current = source;
    Consume(current);
    int out = Forward(current);
}

struct EventEnvelope {
    EventHint hint;
};

EventEnvelope MakeEnvelope(int value) {
    EventEnvelope envelope{MakeHint(value)};
    return envelope;
}

struct NestedEnvelope {
    EventEnvelope envelope;
};

NestedEnvelope MakeNestedEnvelope(int value) {
    NestedEnvelope nested{MakeEnvelope(value)};
    return nested;
}

int ExtractHintPower(const EventHint& hint) {
    return hint.power;
}

void EmitPower(int power) {
    Consume(power);
}

class EnvelopeWorker {
public:
    void ApplyHint(const EventHint& hint) {
        this->stored = hint.power;
    }

    void RunEnvelope(int source) {
        EventEnvelope envelope = MakeEnvelope(source);
        ApplyHint(envelope.hint);
    }

private:
    int stored;
};

class NestedEnvelopeWorker {
public:
    void ApplyHint(const EventHint& hint) {
        this->stored = hint.power;
    }

    void RunNestedEnvelope(int source) {
        NestedEnvelope nested = MakeNestedEnvelope(source);
        ApplyHint(nested.envelope.hint);
    }

private:
    int stored;
};

class MemberRelayWorker {
public:
    void Seed(const EventHint& hint) {
        this->stored = hint.power;
    }

    void EmitStored() {
        EmitPower(this->stored);
    }

    void RunMemberRelay(int source) {
        EventHint staged = MakeHint(source);
        Seed(staged);
        EmitStored();
    }

private:
    int stored;
};

void RelayNestedHint(int source) {
    NestedEnvelope nested = MakeNestedEnvelope(source);
    int power = ExtractHintPower(nested.envelope.hint);
    Consume(power);
}

void RelayNestedHintToEmitter(int source) {
    NestedEnvelope nested = MakeNestedEnvelope(source);
    int power = ExtractHintPower(nested.envelope.hint);
    EmitPower(power);
}
}
