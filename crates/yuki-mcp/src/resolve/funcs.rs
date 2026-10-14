//! No-arg template functions — `{{ $fake.email() }}`, `{{ $uuid.v4() }}`,
//! `{{ $timestamp.iso() }}` — evaluated by `resolve_str`, a fresh value per
//! occurrence. The `$` sigil (as Postman's `{{$guid}}`) tells them apart from
//! variables. `fake.*` ports `field-faker/src/lib/fake.js`.

use rand::RngExt;

const FIRST: &[&str] = &[
    "Ana", "Bruno", "Carla", "Diego", "Elena", "Felipe", "Gabriela", "Hugo", "Isabela", "João",
    "Larissa", "Marcos", "Natália", "Otávio", "Paula", "Rafael", "Sofia", "Thiago", "Camila",
    "Lucas", "Beatriz", "Pedro", "Juliana", "André", "Fernanda", "Ricardo", "Mariana", "Gustavo",
];
const LAST: &[&str] = &[
    "Almeida",
    "Barbosa",
    "Costa",
    "Dias",
    "Fernandes",
    "Gomes",
    "Lima",
    "Martins",
    "Nunes",
    "Oliveira",
    "Pereira",
    "Rocha",
    "Santos",
    "Silva",
    "Souza",
    "Teixeira",
    "Carvalho",
    "Ribeiro",
    "Araujo",
    "Melo",
    "Castro",
    "Cardoso",
    "Correia",
    "Mendes",
    "Nascimento",
    "Moreira",
];
const COMPANIES: &[&str] = &[
    "Acme Ltda",
    "NovaTech",
    "Horizon Labs",
    "Ponta Digital",
    "Campo Verde SA",
    "Atlas Soft",
    "Vértice Sistemas",
    "Lumen Data",
    "Norte Cloud",
    "Foco Analytics",
    "Primeira Linha",
    "Delta Ops",
    "Brasa Code",
    "Serra Apps",
];
const JOBS: &[&str] = &[
    "Desenvolvedor",
    "Analista de Sistemas",
    "Product Manager",
    "Designer UX",
    "Engenheiro de Dados",
    "QA Analyst",
    "DevOps",
    "Tech Lead",
    "Scrum Master",
    "Analista de Negócios",
    "Suporte Técnico",
    "Arquiteto de Software",
];
const STREETS: &[&str] = &[
    "Rua das Flores",
    "Av. Brasil",
    "Rua XV de Novembro",
    "Alameda Santos",
    "Rua Augusta",
    "Av. Paulista",
    "Rua da Consolação",
    "Travessa do Comércio",
    "Rua Sete de Setembro",
    "Av. Atlântica",
    "Rua Oscar Freire",
    "Rua Vergueiro",
];
const CITIES: &[&str] = &[
    "São Paulo",
    "Rio de Janeiro",
    "Belo Horizonte",
    "Curitiba",
    "Porto Alegre",
    "Brasília",
    "Salvador",
    "Recife",
    "Fortaleza",
    "Campinas",
    "Florianópolis",
    "Goiânia",
    "Manaus",
    "Belém",
];
const LOREM: &[&str] = &[
    "Lorem ipsum dolor sit amet.",
    "Consectetur adipiscing elit sed do.",
    "Ut labore et dolore magna aliqua.",
    "Quick brown fox jumps over the lazy dog.",
    "Dados de teste gerados localmente.",
    "Preenchimento automático para QA.",
];
/// Real CEPs: random digits often 404 on ViaCEP.
const CEPS: &[&str] = &[
    "01310-100",
    "22041-080",
    "30130-010",
    "80010-000",
    "90010-150",
    "40020-000",
    "60060-100",
    "69005-070",
    "70040-010",
    "88015-100",
    "13010-111",
    "29010-120",
];

pub type Generator = fn() -> String;

/// Name (after `$`, without `()`) → generator. Also the autocomplete list.
pub const FUNCTIONS: &[(&str, Generator)] = &[
    ("fake.fullName", || {
        format!("{} {}", pick(FIRST), pick(LAST))
    }),
    ("fake.firstName", || pick(FIRST).into()),
    ("fake.lastName", || pick(LAST).into()),
    ("fake.email", || {
        format!(
            "{}.{}{}@example.com",
            slug(pick(FIRST)),
            slug(pick(LAST)),
            digits(2)
        )
    }),
    ("fake.phone", || {
        format!(
            "({}) 9{}-{}",
            rand::rng().random_range(11..100),
            digits(4),
            digits(4)
        )
    }),
    ("fake.cpf", || cpf(random_digits(9))),
    ("fake.cnpj", || cnpj(random_digits(12))),
    ("fake.uuid", uuid),
    ("fake.lorem", || pick(LOREM).into()),
    ("fake.number", || {
        rand::rng().random_range(1..10_000).to_string()
    }),
    ("fake.password", || {
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%";
        let mut rng = rand::rng();
        (0..12)
            .map(|_| char::from(CHARS[rng.random_range(0..CHARS.len())]))
            .collect()
    }),
    ("fake.date", || {
        let mut rng = rand::rng();
        format!(
            "{}-{:02}-{:02}",
            rng.random_range(1970..2005),
            rng.random_range(1..=12),
            rng.random_range(1..=28)
        )
    }),
    ("fake.username", || {
        format!("{}_{}{}", slug(pick(FIRST)), slug(pick(LAST)), digits(2))
    }),
    ("fake.company", || pick(COMPANIES).into()),
    ("fake.street", || {
        format!("{} {}", rand::rng().random_range(100..9000), pick(STREETS))
    }),
    ("fake.city", || pick(CITIES).into()),
    ("fake.zipCode", || pick(CEPS).into()),
    ("fake.jobTitle", || pick(JOBS).into()),
    ("fake.url", || {
        format!("https://www.example-{}.com", digits(3))
    }),
    ("uuid.v4", uuid),
    ("timestamp.iso", || now("%Y-%m-%dT%H:%M:%S%.3fZ")),
    ("timestamp.unix", || {
        chrono::Utc::now().timestamp().to_string()
    }),
    ("timestamp.unixMs", || {
        chrono::Utc::now().timestamp_millis().to_string()
    }),
    ("timestamp.date", || now("%Y-%m-%d")),
    ("timestamp.time", || now("%H:%M:%S")),
];

/// The generator for a `$name()` call; `None` for anything else (unknown
/// names, calls with arguments, Voleeo's unprefixed `uuid.v4()`), which
/// `resolve_str` keeps verbatim.
pub fn lookup(expr: &str) -> Option<Generator> {
    let name = expr.strip_prefix('$')?.strip_suffix("()")?;
    FUNCTIONS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

fn pick(list: &[&'static str]) -> &'static str {
    list[rand::rng().random_range(0..list.len())]
}

fn random_digits(n: usize) -> Vec<u32> {
    let mut rng = rand::rng();
    (0..n).map(|_| rng.random_range(0..10)).collect()
}

fn digits(n: usize) -> String {
    random_digits(n).iter().map(u32::to_string).collect()
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now(fmt: &str) -> String {
    chrono::Utc::now().format(fmt).to_string()
}

/// ASCII, lowercase, `_` between words — names in an email/username.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| match c.to_lowercase().next().unwrap_or(c) {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' => 'e',
            'í' | 'î' => 'i',
            'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_ascii_alphanumeric() => c,
            _ => '_',
        })
        .collect()
}

/// Mod-11 check digit (CPF/CNPJ): `weights` lines up with `n`.
fn push_check(n: &mut Vec<u32>, weights: &[u32]) {
    let r = n.iter().zip(weights).map(|(d, w)| d * w).sum::<u32>() % 11;
    n.push(if r < 2 { 0 } else { 11 - r });
}

fn joined(n: &[u32]) -> String {
    n.iter().map(u32::to_string).collect()
}

/// 9 base digits → `000.000.000-00`.
fn cpf(mut n: Vec<u32>) -> String {
    const W: [u32; 10] = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2];
    push_check(&mut n, &W[1..]);
    push_check(&mut n, &W);
    let s = joined(&n);
    format!("{}.{}.{}-{}", &s[..3], &s[3..6], &s[6..9], &s[9..])
}

/// 12 base digits → `00.000.000/0000-00`.
fn cnpj(mut n: Vec<u32>) -> String {
    const W: [u32; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    push_check(&mut n, &W[1..]);
    push_check(&mut n, &W);
    let s = joined(&n);
    format!(
        "{}.{}.{}/{}-{}",
        &s[..2],
        &s[2..5],
        &s[5..8],
        &s[8..12],
        &s[12..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(s: &str) -> Vec<u32> {
        s.chars().filter_map(|c| c.to_digit(10)).collect()
    }

    #[test]
    fn cpf_cnpj_check_digits() {
        assert_eq!(cpf(base("529982247")), "529.982.247-25");
        assert_eq!(cpf(base("000000001")), "000.000.001-91");
        assert_eq!(cnpj(base("112223330001")), "11.222.333/0001-81");
    }

    #[test]
    fn slug_strips_accents() {
        assert_eq!(slug("João"), "joao");
        assert_eq!(slug("Natália"), "natalia");
    }
}
