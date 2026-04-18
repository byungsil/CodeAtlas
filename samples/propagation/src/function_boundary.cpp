namespace Game {
void Consume(int value) {}

int Forward(int value) {
    int local = value;
    return local;
}

void Tick(int source) {
    int current = source;
    Consume(current);
    int out = Forward(current);
}
}
