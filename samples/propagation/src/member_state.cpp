namespace Game {
struct Carrier {
    int power;
    int mirrored;
};

struct CarrierEnvelope {
    Carrier carrier;
};

Carrier MakeCarrier(int value) {
    Carrier carrier{value, value};
    return carrier;
}

CarrierEnvelope MakeCarrierEnvelope(int value) {
    CarrierEnvelope envelope{MakeCarrier(value)};
    return envelope;
}

class Worker {
public:
    void SetFromParam(int value) {
        this->stored = value;
    }

    void SetFromLocal(int value) {
        int local = value;
        this->cached = local;
    }

    int ReadToLocal() {
        int local = this->stored;
        return local;
    }

    int ReadMember() {
        return this->cached;
    }

    void CopyToPeer(Worker& other) {
        other.shared = this->cached;
    }

    int ReadFromPointer(Worker* other) {
        int local = other->shared;
        return local;
    }

    void PullFromCarrier(const Carrier& carrier) {
        this->stored = carrier.power;
    }

    void PushToCarrier(Carrier& carrier) {
        carrier.mirrored = this->cached;
    }

    void StageThroughLocalCarrier(int value) {
        Carrier stage;
        stage.power = value;
        this->stored = stage.power;
    }

private:
    int stored;
    int cached;
    int shared;
};

class ConstructedWorker {
public:
    ConstructedWorker(int initialStored, int initialCached)
        : stored(initialStored), cached{initialCached} {}

private:
    int stored;
    int cached;
};

class CarrierConstructedWorker {
public:
    CarrierConstructedWorker(const Carrier& carrier)
        : stored(carrier.power), cached{carrier.mirrored} {}

private:
    int stored;
    int cached;
};

class PointerCarrierConstructedWorker {
public:
    PointerCarrierConstructedWorker(const Carrier* carrier)
        : stored(carrier->power), cached{carrier->mirrored} {}

private:
    int stored;
    int cached;
};

class HelperCarrierConstructedWorker {
public:
    HelperCarrierConstructedWorker(int value)
        : stored(MakeCarrier(value).power), cached{MakeCarrier(value).mirrored} {}

private:
    int stored;
    int cached;
};

class HelperCarrierPipelineWorker {
public:
    HelperCarrierPipelineWorker(int value)
        : stored(MakeCarrier(value).power) {}

    int EmitStored() {
        int current = this->stored;
        return current;
    }

private:
    int stored;
};

class NestedHelperCarrierPipelineWorker {
public:
    NestedHelperCarrierPipelineWorker(int value)
        : stored(MakeCarrierEnvelope(value).carrier.power) {}

    int EmitStored() {
        int current = this->stored;
        return current;
    }

private:
    int stored;
};
}
