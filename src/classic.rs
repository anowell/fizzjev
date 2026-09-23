pub fn fizzbuzz(n: u32) -> String {
    match (n % 3, n % 5) {
        (0, 0) => "FizzBuzz".to_string(),
        (0, _) => "Fizz".to_string(),
        (_, 0) => "Buzz".to_string(),
        _ => n.to_string(),
    }
}

pub fn words(n: u32) -> String {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    match n {
        0..=19 => ONES[n as usize].to_string(),
        20..=99 if n.is_multiple_of(10) => TENS[(n / 10) as usize].to_string(),
        20..=99 => format!("{}-{}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]),
        100..=999 if n.is_multiple_of(100) => format!("{} hundred", ONES[(n / 100) as usize]),
        100..=999 => format!("{} hundred {}", ONES[(n / 100) as usize], words(n % 100)),
        1000..=999_999 if n.is_multiple_of(1000) => format!("{} thousand", words(n / 1000)),
        1000..=999_999 => format!("{} thousand {}", words(n / 1000), words(n % 1000)),
        _ => n.to_string(),
    }
}

/// Least significant first, as in the 2016 post.
pub fn binary_digits(n: u32, bits: u32) -> Vec<u8> {
    (0..bits).map(|d| ((n >> d) & 1) as u8).collect()
}

/// Never fewer than the post's ten bits.
pub fn bits_for(max: u32) -> u32 {
    (u32::BITS - max.leading_zeros()).max(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boring_one_is_correct() {
        let got: Vec<String> = (1..=15).map(fizzbuzz).collect();
        let want = [
            "1", "2", "Fizz", "4", "Buzz", "Fizz", "7", "8", "Fizz", "Buzz", "11", "Fizz", "13",
            "14", "FizzBuzz",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn numbers_become_words() {
        assert_eq!(words(7), "seven");
        assert_eq!(words(15), "fifteen");
        assert_eq!(words(40), "forty");
        assert_eq!(words(42), "forty-two");
        assert_eq!(words(100), "one hundred");
        assert_eq!(words(101), "one hundred one");
        assert_eq!(words(1024), "one thousand twenty-four");
        assert_eq!(words(9000), "nine thousand");
        assert_eq!(words(10999), "ten thousand nine hundred ninety-nine");
        assert_eq!(
            words(123_456),
            "one hundred twenty-three thousand four hundred fifty-six"
        );
    }

    #[test]
    fn binary_is_little_endian() {
        assert_eq!(binary_digits(5, 10), [1, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(binary_digits(1023, 10).iter().sum::<u8>(), 10);
        assert_eq!(bits_for(100), 10);
        assert_eq!(bits_for(1023), 10);
        assert_eq!(bits_for(1024), 11);
        assert_eq!(bits_for(10999), 14);
    }
}
