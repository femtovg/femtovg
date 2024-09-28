use femtovg::Transform2D;

#[test]
fn test_multiplication() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(25.0, 28.0, 57.0, 64.0, 100.0, 112.0);

    let result = a * b;
    assert_eq!(result, expected);
}

#[test]
fn test_multiplication_assignment() {
    let mut a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(25.0, 28.0, 57.0, 64.0, 100.0, 112.0);

    a *= b;
    assert_eq!(a, expected);
}

#[test]
fn test_premultiply() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);

    let mut premultiplied = a;
    premultiplied.premultiply(&b);

    assert_eq!(premultiplied, b * a);
}

#[test]
fn test_division() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(-2.0, 3.0, -3.0, 4.0, -3.0, 3.0);

    let result = b / a;
    assert_eq!(result, expected);
}

#[test]
fn test_division_assignment() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let mut b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(-2.0, 3.0, -3.0, 4.0, -3.0, 3.0);

    b /= a;
    assert_eq!(b, expected);
}
