namespace Game {
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

private:
    int stored;
    int cached;
    int shared;
};
}
