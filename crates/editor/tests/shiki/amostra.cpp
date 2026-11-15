#include <iostream>
#include <vector>
#include <string>

namespace exemplo {

// Classe template
template <typename T>
class Pilha : public Base {
public:
    explicit Pilha(std::size_t n) : dados_(n) {}
    virtual ~Pilha() = default;
    void empilhar(const T& v) { dados_.push_back(v); }
    T topo() const noexcept { return dados_.back(); }
private:
    std::vector<T> dados_;
};

}  // namespace exemplo

int main() {
    auto p = exemplo::Pilha<int>(10);
    p.empilhar(42);
    std::string s = "valor";
    constexpr double pi = 3.14159;
    std::cout << s << p.topo() << pi << nullptr << true << std::endl;
    return 0;
}
