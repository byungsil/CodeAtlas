namespace Game {
void Tick(int input) {
    int first = input;
    int second = first;
    second = input;
    int third = second = first;
    auto copy = third;
    int* ptr = &copy;
    int alias = *ptr;
}
}
